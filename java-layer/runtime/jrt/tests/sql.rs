//! java.sql on SQLite, the way plugins use it: open, create, insert with parameters and batches, read back,
//! transactions, HikariCP, and the error a MySQL URL gives with no driver.
use jrt::{s, V};

fn ok(r: jrt::R) -> V {
    match r {
        Ok(v) => v,
        Err(e) => panic!("{}", e.text()),
    }
}

fn call(this: &V, name: &str, a: &[V]) -> V {
    ok(jrt::sql::method(this, name, a).expect(name))
}

#[test]
fn sqlite_like_a_plugin() {
    let path = std::env::temp_dir().join(format!("jrt-sql-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let url = format!("jdbc:sqlite:{}", path.display());
    let conn = jrt::sql::static_call("DriverManager", "getConnection", &[s(&url)]).map(ok).unwrap();
    let st = call(&conn, "createStatement", &[]);
    call(&st, "executeUpdate", &[s("CREATE TABLE IF NOT EXISTS homes (uuid TEXT, name TEXT, x DOUBLE, count INTEGER)")]);

    let insert = call(&conn, "prepareStatement", &[s("INSERT INTO homes VALUES (?, ?, ?, ?)")]);
    call(&insert, "setString", &[V::Int(1), s("abc")]);
    call(&insert, "setString", &[V::Int(2), s("base")]);
    call(&insert, "setDouble", &[V::Int(3), V::Double(12.5)]);
    call(&insert, "setInt", &[V::Int(4), V::Int(3)]);
    assert_eq!(call(&insert, "executeUpdate", &[]).int(), 1);

    // A batch in a transaction, committed; then one rolled back.
    call(&conn, "setAutoCommit", &[V::Bool(false)]);
    for name in ["farm", "mine"] {
        call(&insert, "setString", &[V::Int(1), s("abc")]);
        call(&insert, "setString", &[V::Int(2), s(name)]);
        call(&insert, "setDouble", &[V::Int(3), V::Double(1.0)]);
        call(&insert, "setInt", &[V::Int(4), V::Int(1)]);
        call(&insert, "addBatch", &[]);
    }
    assert_eq!(call(&insert, "executeBatch", &[]).items().ok().unwrap().len(), 2);
    call(&conn, "commit", &[]);
    call(&st, "executeUpdate", &[s("DELETE FROM homes")]);
    call(&conn, "rollback", &[]);
    call(&conn, "setAutoCommit", &[V::Bool(true)]);

    let query = call(&conn, "prepareStatement", &[s("SELECT name, x, count FROM homes WHERE uuid = ? ORDER BY name")]);
    call(&query, "setString", &[V::Int(1), s("abc")]);
    let rows = call(&query, "executeQuery", &[]);
    let mut seen = Vec::new();
    while call(&rows, "next", &[]).truth() {
        seen.push((call(&rows, "getString", &[s("name")]).text(), call(&rows, "getDouble", &[V::Int(2)]).double(), call(&rows, "getInt", &[s("COUNT")]).int()));
    }
    assert_eq!(seen, vec![("base".into(), 12.5, 3), ("farm".into(), 1.0, 1), ("mine".into(), 1.0, 1)]);
    call(&conn, "close", &[]);
    assert!(call(&conn, "isClosed", &[]).truth());

    // HikariCP in front of the same file.
    let config = jrt::sql::construct("HikariConfig", &[]).map(ok).unwrap();
    call(&config, "setJdbcUrl", &[s(&url)]);
    call(&config, "setMaximumPoolSize", &[V::Int(10)]);
    let pool = jrt::sql::construct("HikariDataSource", &[config]).map(ok).unwrap();
    let pooled = call(&pool, "getConnection", &[]);
    let count = call(&call(&call(&pooled, "createStatement", &[]), "executeQuery", &[s("SELECT COUNT(*) AS n FROM homes")]), "getMetaData", &[]);
    assert_eq!(call(&count, "getColumnCount", &[]).int(), 1);

    // No MySQL driver: the error Java gives.
    let mysql = jrt::sql::static_call("DriverManager", "getConnection", &[s("jdbc:mysql://localhost/db")]).unwrap();
    assert!(mysql.is_err());
    let _ = std::fs::remove_file(&path);
}
