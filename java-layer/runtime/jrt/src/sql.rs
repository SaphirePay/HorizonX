//! `java.sql` on SQLite (`DriverManager.getConnection("jdbc:sqlite:...")`) and, with the `mysql` feature, on MySQL /
//! MariaDB (`jdbc:mysql://host:port/db`, `jdbc:mariadb://...`, with the user and password given): statements,
//! prepared statements, result sets, transactions and batches, and HikariCP in front of them. Any other URL answers
//! as Java does with no driver for it: `SQLException: No suitable driver found`.
use super::*;
use rusqlite::types::Value as Sql;

/// The database a connection talks to.
enum Db {
    Lite(rusqlite::Connection),
    #[cfg(feature = "mysql")]
    My(mysql::Conn),
}

/// `Connection`. Manual commit is an open transaction, started again after each commit or rollback.
pub struct JConn {
    db: Mutex<Option<Db>>,
    manual: Mutex<bool>,
    url: String,
}

/// `Statement`, and `PreparedStatement` when it carries its SQL and parameters.
pub struct JStmt {
    conn: V,
    sql: Option<String>,
    params: Mutex<Vec<Sql>>,
    batch: Mutex<Vec<Batched>>,
    last_rowid: Mutex<Option<i64>>,
}

enum Batched {
    Text(String),
    Params(Vec<Sql>),
}

/// `ResultSet`: the rows read at once, and a cursor before the first.
pub struct JRows {
    columns: Vec<String>,
    rows: Vec<Vec<Sql>>,
    at: Mutex<isize>,
    was_null: Mutex<bool>,
}

/// `ResultSetMetaData`.
pub struct JMeta(Vec<String>);

/// `DatabaseMetaData`: of its connection, whose tables and columns it reads from the database's catalog.
pub struct JDbMeta(V);

fn empty_rows() -> JRows {
    JRows { columns: Vec::new(), rows: Vec::new(), at: Mutex::new(-1), was_null: Mutex::new(false) }
}

fn is_mysql(conn: &V) -> bool {
    conn.native::<JConn>().is_some_and(|c| !c.url.starts_with("jdbc:sqlite:"))
}

fn db_meta_method(meta: &JDbMeta, name: &str, a: &[V]) -> Option<R> {
    let pattern = |at: usize| a.get(at).filter(|v| !v.is_null()).map_or_else(|| "%".to_owned(), V::text);
    let rows = |sql: &str, params: &[Sql]| match run(&meta.0, sql, params) {
        Ok((rows, ..)) => Ok(native(rows.unwrap_or_else(empty_rows))),
        Err(error) => Err(error),
    };
    let mysql = is_mysql(&meta.0);
    let url = meta.0.native::<JConn>().map(|c| c.url.clone()).unwrap_or_default();
    Some(match name {
        // `getTables(catalog, schemaPattern, tableNamePattern, types)`.
        "getTables" if mysql => rows(
            "SELECT TABLE_CATALOG AS TABLE_CAT, TABLE_SCHEMA AS TABLE_SCHEM, TABLE_NAME, IF(TABLE_TYPE = 'BASE TABLE', 'TABLE', TABLE_TYPE) AS TABLE_TYPE, TABLE_COMMENT AS REMARKS FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME LIKE ? ORDER BY TABLE_NAME",
            &[Sql::Text(pattern(2))],
        ),
        "getTables" => rows(
            "SELECT NULL AS TABLE_CAT, NULL AS TABLE_SCHEM, name AS TABLE_NAME, upper(type) AS TABLE_TYPE, NULL AS REMARKS FROM sqlite_master WHERE type IN ('table', 'view') AND name LIKE ?1 ORDER BY name",
            &[Sql::Text(pattern(2))],
        ),
        // `getColumns(catalog, schemaPattern, tableNamePattern, columnNamePattern)`.
        "getColumns" if mysql => rows(
            "SELECT TABLE_CATALOG AS TABLE_CAT, TABLE_SCHEMA AS TABLE_SCHEM, TABLE_NAME, COLUMN_NAME, UPPER(DATA_TYPE) AS TYPE_NAME, ORDINAL_POSITION, IS_NULLABLE, COLUMN_DEFAULT AS COLUMN_DEF FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME LIKE ? AND COLUMN_NAME LIKE ? ORDER BY TABLE_NAME, ORDINAL_POSITION",
            &[Sql::Text(pattern(2)), Sql::Text(pattern(3))],
        ),
        "getColumns" => rows(
            "SELECT NULL AS TABLE_CAT, NULL AS TABLE_SCHEM, m.name AS TABLE_NAME, p.name AS COLUMN_NAME, p.type AS TYPE_NAME, p.cid + 1 AS ORDINAL_POSITION, CASE p.\"notnull\" WHEN 0 THEN 'YES' ELSE 'NO' END AS IS_NULLABLE, p.dflt_value AS COLUMN_DEF FROM sqlite_master m JOIN pragma_table_info(m.name) p WHERE m.type = 'table' AND m.name LIKE ?1 AND p.name LIKE ?2 ORDER BY m.name, p.cid",
            &[Sql::Text(pattern(2)), Sql::Text(pattern(3))],
        ),
        "getPrimaryKeys" if mysql => rows(
            "SELECT TABLE_CATALOG AS TABLE_CAT, TABLE_SCHEMA AS TABLE_SCHEM, TABLE_NAME, COLUMN_NAME, ORDINAL_POSITION AS KEY_SEQ, CONSTRAINT_NAME AS PK_NAME FROM information_schema.KEY_COLUMN_USAGE WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME LIKE ? AND CONSTRAINT_NAME = 'PRIMARY' ORDER BY ORDINAL_POSITION",
            &[Sql::Text(pattern(2))],
        ),
        "getPrimaryKeys" => rows(
            "SELECT NULL AS TABLE_CAT, NULL AS TABLE_SCHEM, m.name AS TABLE_NAME, p.name AS COLUMN_NAME, p.pk AS KEY_SEQ, NULL AS PK_NAME FROM sqlite_master m JOIN pragma_table_info(m.name) p WHERE m.type = 'table' AND m.name LIKE ?1 AND p.pk > 0 ORDER BY p.pk",
            &[Sql::Text(pattern(2))],
        ),
        "getIndexInfo" if mysql => rows(
            "SELECT TABLE_CATALOG AS TABLE_CAT, TABLE_SCHEMA AS TABLE_SCHEM, TABLE_NAME, NON_UNIQUE, INDEX_NAME, COLUMN_NAME, SEQ_IN_INDEX AS ORDINAL_POSITION FROM information_schema.STATISTICS WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME LIKE ? ORDER BY INDEX_NAME, SEQ_IN_INDEX",
            &[Sql::Text(pattern(2))],
        ),
        "getIndexInfo" => rows(
            "SELECT NULL AS TABLE_CAT, NULL AS TABLE_SCHEM, m.name AS TABLE_NAME, CASE l.\"unique\" WHEN 0 THEN 1 ELSE 0 END AS NON_UNIQUE, l.name AS INDEX_NAME, i.name AS COLUMN_NAME, i.seqno + 1 AS ORDINAL_POSITION FROM sqlite_master m JOIN pragma_index_list(m.name) l JOIN pragma_index_info(l.name) i WHERE m.type = 'table' AND m.name LIKE ?1",
            &[Sql::Text(pattern(2))],
        ),
        "getConnection" => Ok(meta.0.clone()),
        "getDatabaseProductName" => Ok(s(if mysql { "MySQL" } else { "SQLite" })),
        "getDatabaseProductVersion" if mysql => rows("SELECT VERSION()", &[]).map(|r| r.native::<JRows>().and_then(|r| r.rows.first().and_then(|row| row.first()).map(|v| s(&text_of(v)))).unwrap_or_else(|| s(""))),
        "getDatabaseProductVersion" => Ok(s(rusqlite::version())),
        "getDriverName" => Ok(s(if mysql { "MySQL Connector/J" } else { "SQLite JDBC" })),
        "getDriverVersion" => Ok(s(if mysql { "8" } else { "3" })),
        "getDatabaseMajorVersion" | "getDriverMajorVersion" => Ok(V::Int(if mysql { 8 } else { 3 })),
        "getDatabaseMinorVersion" | "getDriverMinorVersion" => Ok(V::Int(0)),
        "getURL" => Ok(s(&url)),
        "getIdentifierQuoteString" => Ok(s(if mysql { "`" } else { "\"" })),
        "supportsBatchUpdates" | "supportsTransactions" | "supportsGetGeneratedKeys" | "supportsSavepoints" => Ok(V::Bool(true)),
        "storesLowerCaseIdentifiers" | "storesUpperCaseIdentifiers" | "supportsSchemasInTableDefinitions" => Ok(V::Bool(false)),
        "storesMixedCaseIdentifiers" => Ok(V::Bool(true)),
        _ => return None,
    })
}

/// `HikariConfig` and `HikariDataSource`: the URL, and the user and password a MySQL one logs in with.
pub struct JHikari {
    url: Mutex<String>,
    user: Mutex<Option<String>>,
    pass: Mutex<Option<String>>,
    closed: Mutex<bool>,
}

fn hikari(url: String, user: Option<String>, pass: Option<String>) -> V {
    native(JHikari { url: Mutex::new(url), user: Mutex::new(user), pass: Mutex::new(pass), closed: Mutex::new(false) })
}

/// `x instanceof Connection` and the like, for these objects: a plugin's overloads tell `prepare(Connection, int)`
/// from `prepare(Connection, String)` by it (CoreProtect's `prepareStatement` took the int one with the SQL text,
/// looked it up in its query map and got null: every insert failed).
pub fn is_instance(value: &V, name: &str) -> bool {
    let name = name.rsplit('.').next().unwrap_or(name);
    if value.native::<JConn>().is_some() {
        return matches!(name, "Connection" | "AutoCloseable" | "Wrapper");
    }
    if value.native::<JStmt>().is_some() {
        return matches!(name, "Statement" | "PreparedStatement" | "CallableStatement" | "AutoCloseable" | "Wrapper");
    }
    if value.native::<JRows>().is_some() {
        return matches!(name, "ResultSet" | "AutoCloseable" | "Wrapper");
    }
    if value.native::<JHikari>().is_some() {
        return matches!(name, "DataSource" | "HikariDataSource" | "HikariConfig" | "SQLiteDataSource" | "CommonDataSource" | "Closeable" | "AutoCloseable" | "Wrapper");
    }
    if value.native::<JDbMeta>().is_some() {
        return matches!(name, "DatabaseMetaData" | "Wrapper");
    }
    value.native::<JMeta>().is_some() && matches!(name, "ResultSetMetaData" | "Wrapper")
}

fn sql_error(error: impl std::fmt::Display) -> V {
    exc("SQLException", error.to_string())
}

fn connection(db: Db, url: &str) -> V {
    native(JConn { db: Mutex::new(Some(db)), manual: Mutex::new(false), url: url.to_owned() })
}

fn open(url: &str, user: Option<String>, pass: Option<String>) -> R {
    if let Some(path) = url.strip_prefix("jdbc:sqlite:") {
        let path = path.split('?').next().unwrap_or("");
        let db = if path.is_empty() || path == ":memory:" {
            rusqlite::Connection::open_in_memory()
        } else {
            if let Some(parent) = std::path::Path::new(path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            rusqlite::Connection::open(path)
        }
        .map_err(sql_error)?;
        let _ = db.busy_timeout(std::time::Duration::from_secs(5));
        return Ok(connection(Db::Lite(db), url));
    }
    if url.starts_with("jdbc:mysql://") || url.starts_with("jdbc:mariadb://") {
        return open_mysql(url, user, pass);
    }
    Err(sql_error(format!("No suitable driver found for {url}")))
}

/// `jdbc:mysql://host[:port][/database][?user=..&password=..&...]`: the user and password given beside it win.
#[cfg(feature = "mysql")]
fn open_mysql(url: &str, user: Option<String>, pass: Option<String>) -> R {
    let rest = url.split_once("://").map_or("", |(_, rest)| rest);
    let (address, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (host_port, database) = address.split_once('/').unwrap_or((address, ""));
    let host_port = host_port.split(',').next().unwrap_or(host_port);
    let (host, port) = match host_port.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => (host, port.parse().unwrap_or(3306)),
        _ => (if host_port.is_empty() { "localhost" } else { host_port }, 3306),
    };
    let param = |key: &str| query.split('&').find_map(|pair| pair.split_once('=').filter(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.to_owned()));
    let user = user.or_else(|| param("user"));
    let pass = pass.or_else(|| param("password"));
    let mut opts = mysql::OptsBuilder::new()
        .ip_or_hostname(Some(host))
        .tcp_port(port)
        .db_name((!database.is_empty()).then_some(database))
        .user(user)
        .pass(pass)
        .tcp_connect_timeout(Some(std::time::Duration::from_secs(10)));
    // `useSSL=true` / `sslMode=REQUIRED`: TLS without checking the certificate, as Connector/J's default trust does.
    if param("useSSL").is_some_and(|v| v.eq_ignore_ascii_case("true")) || param("sslMode").is_some_and(|v| v.eq_ignore_ascii_case("REQUIRED")) {
        opts = opts.ssl_opts(Some(mysql::SslOpts::default().with_danger_accept_invalid_certs(true)));
    }
    let conn = mysql::Conn::new(opts).map_err(|error| exc("SQLNonTransientConnectionException", format!("Could not connect to {host}:{port}: {error}")))?;
    Ok(connection(Db::My(conn), url))
}

#[cfg(not(feature = "mysql"))]
fn open_mysql(url: &str, _user: Option<String>, _pass: Option<String>) -> R {
    Err(sql_error(format!("No suitable driver found for {url}")))
}

fn to_sql(value: &V) -> Sql {
    match value {
        V::Null => Sql::Null,
        V::Bool(b) => Sql::Integer(*b as i64),
        V::Int(i) => Sql::Integer(*i as i64),
        V::Long(i) => Sql::Integer(*i),
        V::Double(d) => Sql::Real(*d),
        V::Char(c) => Sql::Text(c.to_string()),
        V::Arr(_) => match value.items() {
            // byte[] as a blob.
            Ok(items) if items.iter().all(|i| matches!(i, V::Int(_))) => Sql::Blob(items.iter().map(|i| i.int() as u8).collect()),
            _ => Sql::Text(value.text()),
        },
        other => Sql::Text(other.text()),
    }
}

fn from_sql(value: &Sql) -> V {
    match value {
        Sql::Null => V::Null,
        Sql::Integer(i) => V::Long(*i),
        Sql::Real(d) => V::Double(*d),
        Sql::Text(t) => s(t),
        Sql::Blob(b) => arr(b.iter().map(|x| V::Int(*x as i8 as i32)).collect()),
    }
}

fn text_of(value: &Sql) -> String {
    match value {
        Sql::Null => String::new(),
        Sql::Integer(i) => i.to_string(),
        Sql::Real(d) => d.to_string(),
        Sql::Text(t) => t.clone(),
        Sql::Blob(b) => String::from_utf8_lossy(b).into_owned(),
    }
}

fn with_db<T>(conn: &V, work: impl FnOnce(&mut Db) -> Result<T, V>) -> Result<T, V> {
    let c = conn.native::<JConn>().ok_or_else(|| sql_error("not a connection"))?;
    let mut guard = lock(&c.db);
    let db = guard.as_mut().ok_or_else(|| sql_error("The connection is closed"))?;
    work(db)
}

/// A statement without parameters or rows: `BEGIN`, `COMMIT`, `ROLLBACK`.
fn plain(db: &mut Db, sql: &str) -> Result<(), V> {
    match db {
        Db::Lite(db) => db.execute_batch(sql).map_err(sql_error),
        #[cfg(feature = "mysql")]
        Db::My(conn) => {
            use mysql::prelude::Queryable;
            conn.query_drop(sql).map_err(sql_error)
        }
    }
}

/// Runs one statement: rows when it gives any (a SELECT), else the count of rows it changed.
fn run(conn: &V, sql: &str, params: &[Sql]) -> Result<(Option<JRows>, i64, i64), V> {
    with_db(conn, |db| match db {
        Db::Lite(db) => {
            let mut stmt = db.prepare(sql).map_err(sql_error)?;
            let bound: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
            if stmt.column_count() > 0 {
                let columns: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();
                let width = columns.len();
                let mut rows = Vec::new();
                let mut cursor = stmt.query(bound.as_slice()).map_err(sql_error)?;
                while let Some(row) = cursor.next().map_err(sql_error)? {
                    rows.push((0..width).map(|i| row.get::<_, Sql>(i).unwrap_or(Sql::Null)).collect());
                }
                Ok((Some(JRows { columns, rows, at: Mutex::new(-1), was_null: Mutex::new(false) }), -1, db.last_insert_rowid()))
            } else {
                let changed = stmt.execute(bound.as_slice()).map_err(sql_error)?;
                Ok((None, changed as i64, db.last_insert_rowid()))
            }
        }
        #[cfg(feature = "mysql")]
        Db::My(conn) => run_mysql(conn, sql, params),
    })
}

#[cfg(feature = "mysql")]
fn run_mysql(conn: &mut mysql::Conn, sql: &str, params: &[Sql]) -> Result<(Option<JRows>, i64, i64), V> {
    use mysql::prelude::Queryable;
    let to_my = |value: &Sql| match value {
        Sql::Null => mysql::Value::NULL,
        Sql::Integer(i) => mysql::Value::Int(*i),
        Sql::Real(d) => mysql::Value::Double(*d),
        Sql::Text(t) => mysql::Value::Bytes(t.clone().into_bytes()),
        Sql::Blob(b) => mysql::Value::Bytes(b.clone()),
    };
    // Bytes of a binary column (charset 63) stay bytes (CoreProtect's block metadata); any other are text.
    let from_my = |value: mysql::Value, binary: bool| match value {
        mysql::Value::NULL => Sql::Null,
        mysql::Value::Int(i) => Sql::Integer(i),
        mysql::Value::UInt(u) => Sql::Integer(u as i64),
        mysql::Value::Float(f) => Sql::Real(f64::from(f)),
        mysql::Value::Double(d) => Sql::Real(d),
        mysql::Value::Bytes(b) if binary => Sql::Blob(b),
        mysql::Value::Bytes(b) => match String::from_utf8(b) {
            Ok(text) => Sql::Text(text),
            Err(e) => Sql::Blob(e.into_bytes()),
        },
        mysql::Value::Date(y, mo, d, h, mi, s, us) => Sql::Text(format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}{}", if us > 0 { format!(".{us:06}") } else { String::new() })),
        mysql::Value::Time(neg, d, h, mi, s, _) => Sql::Text(format!("{}{:02}:{mi:02}:{s:02}", if neg { "-" } else { "" }, u32::from(h) + d * 24)),
    };
    let collect = |result: &mut dyn FnMut() -> Result<(Vec<(String, bool)>, Vec<Vec<mysql::Value>>, u64, Option<u64>), mysql::Error>| -> Result<(Option<JRows>, i64, i64), V> {
        let (columns, rows, affected, last) = result().map_err(sql_error)?;
        let last = last.unwrap_or(0) as i64;
        if columns.is_empty() {
            return Ok((None, affected as i64, last));
        }
        let rows = rows.into_iter().map(|row| row.into_iter().zip(&columns).map(|(value, (_, binary))| from_my(value, *binary)).collect()).collect();
        Ok((Some(JRows { columns: columns.into_iter().map(|(name, _)| name).collect(), rows, at: Mutex::new(-1), was_null: Mutex::new(false) }), -1, last))
    };
    macro_rules! gather {
        ($result:expr) => {{
            let mut result = $result?;
            let columns: Vec<(String, bool)> = result.columns().as_ref().iter().map(|c| (c.name_str().to_string(), c.character_set() == 63)).collect();
            let (affected, last) = (result.affected_rows(), result.last_insert_id());
            let mut rows = Vec::new();
            if let Some(set) = result.iter() {
                for row in set {
                    let row: mysql::Row = row?;
                    rows.push(row.unwrap());
                }
            }
            Ok((columns, rows, affected, last))
        }};
    }
    if params.is_empty() {
        // The text protocol: DDL and anything a server won't prepare.
        collect(&mut || gather!(conn.query_iter(sql)))
    } else {
        let values: Vec<mysql::Value> = params.iter().map(to_my).collect();
        collect(&mut || gather!(conn.exec_iter(sql, mysql::Params::Positional(values.clone()))))
    }
}

fn statement(conn: &V, sql: Option<String>) -> V {
    native(JStmt { conn: conn.clone(), sql, params: Mutex::new(Vec::new()), batch: Mutex::new(Vec::new()), last_rowid: Mutex::new(None) })
}

pub fn construct(class: &str, a: &[V]) -> Option<R> {
    Some(Ok(match (class, a.len()) {
        ("HikariConfig", _) | ("HikariDataSource", 0) | ("SQLiteDataSource", 0) | ("MysqlDataSource", 0) | ("MariaDbDataSource", 0) => hikari(String::new(), None, None),
        ("HikariDataSource", 1) => match a[0].native::<JHikari>() {
            Some(h) => hikari(lock(&h.url).clone(), lock(&h.user).clone(), lock(&h.pass).clone()),
            None => hikari(String::new(), None, None),
        },
        _ => return None,
    }))
}

pub fn static_call(class: &str, name: &str, a: &[V]) -> Option<R> {
    match (class, name) {
        // `getConnection(url)`, `(url, user, password)`, `(url, Properties)` with `user` and `password` in it.
        ("DriverManager", "getConnection") => {
            let (user, pass) = match a {
                [_, user, pass] => ((!user.is_null()).then(|| user.text()), (!pass.is_null()).then(|| pass.text())),
                [_, props] if !props.is_null() => {
                    let get = |key: &str| call(props, "getProperty", &[s(key)]).ok().filter(|v| !v.is_null()).map(|v| v.text());
                    (get("user"), get("password"))
                }
                _ => (None, None),
            };
            Some(open(&a[0].text(), user, pass))
        }
        ("DriverManager", "setLoginTimeout" | "registerDriver") => Some(Ok(V::Null)),
        _ => None,
    }
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if let Some(h) = this.native::<JHikari>() {
        return Some(Ok(match name {
            "setJdbcUrl" | "setUrl" | "setURL" => {
                *lock(&h.url) = a[0].text();
                V::Null
            }
            "getJdbcUrl" | "getUrl" | "getURL" => s(&lock(&h.url)),
            "setUsername" | "setUser" => {
                *lock(&h.user) = (!a[0].is_null()).then(|| a[0].text());
                V::Null
            }
            "getUsername" | "getUser" => lock(&h.user).as_deref().map_or(V::Null, s),
            "setPassword" => {
                *lock(&h.pass) = (!a[0].is_null()).then(|| a[0].text());
                V::Null
            }
            // `getConnection()` on the pool's own credentials; `getConnection(user, password)` on those given.
            "getConnection" => {
                let (user, pass) = match a {
                    [user, pass] => (Some(user.text()), Some(pass.text())),
                    _ => (lock(&h.user).clone(), lock(&h.pass).clone()),
                };
                return Some(open(&lock(&h.url), user, pass));
            }
            "close" => {
                *lock(&h.closed) = true;
                V::Null
            }
            "isClosed" => V::Bool(*lock(&h.closed)),
            "isRunning" => V::Bool(!*lock(&h.closed)),
            // Pool sizes, timeouts, names, driver classes and data-source properties: nothing to do here.
            _ if name.starts_with("set") || name.starts_with("add") => V::Null,
            "getMaximumPoolSize" | "getMinimumIdle" => V::Int(1),
            _ => return None,
        }));
    }
    if this.native::<JConn>().is_some() {
        let c = this.native::<JConn>()?;
        return Some(match name {
            "createStatement" => Ok(statement(this, None)),
            "prepareStatement" | "prepareCall" => Ok(statement(this, Some(a[0].text()))),
            "setAutoCommit" => {
                let manual = !a[0].truth();
                let was = std::mem::replace(&mut *lock(&c.manual), manual);
                match (was, manual) {
                    (false, true) => with_db(this, |db| plain(db, "BEGIN")).map(|_| V::Null),
                    (true, false) => with_db(this, |db| plain(db, "COMMIT")).map(|_| V::Null),
                    _ => Ok(V::Null),
                }
            }
            "getAutoCommit" => Ok(V::Bool(!*lock(&c.manual))),
            "commit" | "rollback" => {
                let word = if name == "commit" { "COMMIT" } else { "ROLLBACK" };
                let again = *lock(&c.manual);
                with_db(this, |db| {
                    let open = match db {
                        Db::Lite(db) => !db.is_autocommit(),
                        #[cfg(feature = "mysql")]
                        Db::My(_) => true,
                    };
                    if open {
                        plain(db, word)?;
                    }
                    if again {
                        plain(db, "BEGIN")?;
                    }
                    Ok(V::Null)
                })
            }
            "close" | "abort" => {
                if let Some(Db::Lite(db)) = lock(&c.db).take() {
                    let _ = db.close();
                }
                Ok(V::Null)
            }
            "isClosed" => Ok(V::Bool(lock(&c.db).is_none())),
            "isValid" => Ok(V::Bool(lock(&c.db).is_some())),
            "getMetaData" => Ok(native(JDbMeta(this.clone()))),
            "setTransactionIsolation" | "setReadOnly" | "clearWarnings" | "setNetworkTimeout" | "setCatalog" | "setSchema" => Ok(V::Null),
            "getWarnings" => Ok(V::Null),
            _ => return None,
        });
    }
    if let Some(st) = this.native::<JStmt>() {
        let text = |at: usize| a.get(at).map(|v| v.text());
        return Some(match name {
            "setString" | "setInt" | "setLong" | "setDouble" | "setFloat" | "setBoolean" | "setShort" | "setByte" | "setObject" | "setBytes" | "setBigDecimal" | "setTimestamp" | "setDate" | "setNull" => {
                let at = (a[0].int() - 1).max(0) as usize;
                let value = if name == "setNull" { Sql::Null } else { to_sql(&a[1]) };
                let mut params = lock(&st.params);
                if params.len() <= at {
                    params.resize(at + 1, Sql::Null);
                }
                params[at] = value;
                Ok(V::Null)
            }
            "clearParameters" => {
                lock(&st.params).clear();
                Ok(V::Null)
            }
            "executeQuery" | "executeUpdate" | "execute" | "executeLargeUpdate" => {
                let sql = text(0).or_else(|| st.sql.clone()).unwrap_or_default();
                let params = if a.is_empty() { lock(&st.params).clone() } else { Vec::new() };
                let (rows, changed, rowid) = match run(&st.conn, &sql, &params) {
                    Ok(done) => done,
                    Err(error) => return Some(Err(error)),
                };
                *lock(&st.last_rowid) = Some(rowid);
                Ok(match name {
                    "executeQuery" => native(rows.unwrap_or_else(empty_rows)),
                    "execute" => V::Bool(rows.is_some()),
                    "executeLargeUpdate" => V::Long(changed.max(0)),
                    _ => V::Int(changed.max(0) as i32),
                })
            }
            "addBatch" => {
                let item = match text(0) {
                    Some(sql) => Batched::Text(sql),
                    None => Batched::Params(std::mem::take(&mut *lock(&st.params))),
                };
                lock(&st.batch).push(item);
                Ok(V::Null)
            }
            "executeBatch" => {
                let items = std::mem::take(&mut *lock(&st.batch));
                let mut counts = Vec::new();
                for item in items {
                    let done = match item {
                        Batched::Text(sql) => run(&st.conn, &sql, &[]),
                        Batched::Params(params) => run(&st.conn, st.sql.as_deref().unwrap_or(""), &params),
                    };
                    match done {
                        Ok((_, changed, rowid)) => {
                            *lock(&st.last_rowid) = Some(rowid);
                            counts.push(V::Int(changed.max(0) as i32));
                        }
                        Err(error) => return Some(Err(error)),
                    }
                }
                Ok(arr(counts))
            }
            "clearBatch" => {
                lock(&st.batch).clear();
                Ok(V::Null)
            }
            "getGeneratedKeys" => {
                let rowid = lock(&st.last_rowid).unwrap_or(0);
                let column = if is_mysql(&st.conn) { "GENERATED_KEY" } else { "last_insert_rowid()" };
                Ok(native(JRows { columns: vec![column.into()], rows: vec![vec![Sql::Integer(rowid)]], at: Mutex::new(-1), was_null: Mutex::new(false) }))
            }
            "getConnection" => Ok(st.conn.clone()),
            "close" | "setQueryTimeout" | "setFetchSize" | "setMaxRows" | "closeOnCompletion" => Ok(V::Null),
            "isClosed" => Ok(V::Bool(false)),
            "getUpdateCount" => Ok(V::Int(-1)),
            _ => return None,
        });
    }
    if let Some(rows) = this.native::<JRows>() {
        let column = |key: &V| -> Option<usize> {
            match key {
                V::Int(i) => Some((*i as usize).saturating_sub(1)),
                V::Long(i) => Some((*i as usize).saturating_sub(1)),
                other => {
                    let wanted = other.text();
                    rows.columns.iter().position(|c| c.eq_ignore_ascii_case(&wanted))
                }
            }
        };
        let current = |key: &V| -> Result<Sql, V> {
            let at = *lock(&rows.at);
            let row = rows.rows.get(at.max(0) as usize).filter(|_| at >= 0).ok_or_else(|| sql_error("ResultSet is not on a row"))?;
            let index = column(key).ok_or_else(|| sql_error(format!("no such column: {}", key.text())))?;
            let value = row.get(index).cloned().unwrap_or(Sql::Null);
            *lock(&rows.was_null) = matches!(value, Sql::Null);
            Ok(value)
        };
        return Some(match name {
            "next" => {
                let mut at = lock(&rows.at);
                *at += 1;
                Ok(V::Bool((*at as usize) < rows.rows.len()))
            }
            "close" => Ok(V::Null),
            "isClosed" => Ok(V::Bool(false)),
            "wasNull" => Ok(V::Bool(*lock(&rows.was_null))),
            "isBeforeFirst" => Ok(V::Bool(*lock(&rows.at) < 0 && !rows.rows.is_empty())),
            "getRow" => Ok(V::Int((*lock(&rows.at) + 1) as i32)),
            "findColumn" => Ok(V::Int(column(&a[0]).map_or(0, |i| i as i32 + 1))),
            "getMetaData" => Ok(native(JMeta(rows.columns.clone()))),
            "getString" | "getNString" => current(&a[0]).map(|v| if matches!(v, Sql::Null) { V::Null } else { s(&text_of(&v)) }),
            "getInt" | "getShort" | "getByte" => current(&a[0]).map(|v| V::Int(number(&v) as i32)),
            "getLong" => current(&a[0]).map(|v| V::Long(number(&v) as i64)),
            "getDouble" | "getFloat" | "getBigDecimal" => current(&a[0]).map(|v| V::Double(number(&v))),
            "getBoolean" => current(&a[0]).map(|v| V::Bool(number(&v) != 0.0 || matches!(&v, Sql::Text(t) if t.eq_ignore_ascii_case("true")))),
            "getObject" => current(&a[0]).map(|v| from_sql(&v)),
            "getBytes" => current(&a[0]).map(|v| from_sql(&match v {
                Sql::Text(t) => Sql::Blob(t.into_bytes()),
                other => other,
            })),
            _ => return None,
        });
    }
    if let Some(meta) = this.native::<JDbMeta>() {
        return db_meta_method(meta, name, a);
    }
    if let Some(meta) = this.native::<JMeta>() {
        return Some(Ok(match name {
            "getColumnCount" => V::Int(meta.0.len() as i32),
            "getColumnName" | "getColumnLabel" => s(meta.0.get((a[0].int() as usize).saturating_sub(1)).map_or("", |c| c.as_str())),
            "getDatabaseProductName" => s("SQLite"),
            "getDatabaseProductVersion" => s(rusqlite::version()),
            "getDriverName" => s("SQLite JDBC"),
            _ => return None,
        }));
    }
    None
}

fn number(value: &Sql) -> f64 {
    match value {
        Sql::Integer(i) => *i as f64,
        Sql::Real(d) => *d,
        Sql::Text(t) => t.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    }
}
