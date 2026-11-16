//! `EssentialsConfiguration` on a YAML tree, with Configurate's rules for reading a value: a node
//! that's there but of the wrong kind gives the kind's zero, not the default. Files are written on a
//! worker thread, one at a time, as Java's single thread executor does.
use crate::big_decimal::BigDecimal;
use crate::config_entities::LazyLocation;
use serde_yaml::{Mapping, Number, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};

enum Job {
    Write(PathBuf, String),
    Flush(Sender<()>),
}

static WRITER: Mutex<Option<Sender<Job>>> = Mutex::new(None);
/// Writes queued and not yet on disk: `load` refuses to read past them.
static PENDING: AtomicU64 = AtomicU64::new(0);

fn writer() -> Sender<Job> {
    let mut slot = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(sender) = slot.as_ref() {
        return sender.clone();
    }
    let (sender, receiver) = channel::<Job>();
    std::thread::spawn(move || {
        for job in receiver {
            match job {
                Job::Write(path, text) => {
                    write_atomic(&path, &text);
                    PENDING.fetch_sub(1, Ordering::SeqCst);
                }
                Job::Flush(done) => {
                    let _ = done.send(());
                }
            }
        }
    });
    *slot = Some(sender.clone());
    sender
}

/// Configurate's `AtomicFiles`: a sibling first, then a rename, so that a crash leaves the old file.
fn write_atomic(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let temporary = path.with_extension("tmp");
    let result = std::fs::write(&temporary, text).and_then(|()| std::fs::rename(&temporary, path));
    if let Err(error) = result {
        crate::severe(&format!("Could not save {}: {error}", path.display()));
    }
}

/// `ConfigurationSaveTask`'s queue is empty once this returns.
pub fn flush() {
    let (done, wait) = channel();
    if writer().send(Job::Flush(done)).is_ok() {
        let _ = wait.recv();
    }
}

/// `EssentialsConfiguration.shutdownExecutor`.
pub fn shutdown_executor() {
    flush();
    *WRITER.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn key_text(key: &Value) -> Option<String> {
    match key {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn child<'a>(node: &'a Value, key: &str) -> Option<&'a Value> {
    node.as_mapping()?.iter().find(|(name, _)| key_text(name).as_deref() == Some(key)).map(|(_, value)| value)
}

fn child_mut<'a>(node: &'a mut Value, key: &str) -> &'a mut Value {
    if !node.is_mapping() {
        *node = Value::Mapping(Mapping::new());
    }
    let Value::Mapping(map) = node else {
        unreachable!("made a mapping above");
    };
    let name = map.keys().find(|name| key_text(name).as_deref() == Some(key)).cloned().unwrap_or_else(|| Value::String(key.to_owned()));
    map.entry(name).or_insert(Value::Null)
}

fn split(path: &str) -> impl Iterator<Item = &str> {
    path.strip_prefix('.').unwrap_or(path).split('.')
}

/// A node by its dotted path; `None` is Configurate's "virtual".
pub fn node_at<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    split(path).try_fold(root, child).filter(|node| !node.is_null())
}

/// `getString`: a scalar's text, or nothing.
pub fn scalar_string(node: &Value) -> Option<String> {
    match node {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(java_number(number)),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn java_number(number: &Number) -> String {
    use horizon_bukkit::JavaString;
    match number.as_i64() {
        Some(value) => value.to_string(),
        None => number.as_f64().map_or_else(|| number.to_string(), |value| value.java_string()),
    }
}

/// Configurate's `Scalars.BOOLEAN`.
pub fn scalar_bool(node: &Value) -> bool {
    match node {
        Value::Bool(flag) => *flag,
        Value::String(text) => matches!(text.to_ascii_lowercase().as_str(), "true" | "yes" | "y" | "t" | "1"),
        Value::Number(number) => number.as_f64().is_some_and(|value| value != 0.0),
        _ => false,
    }
}

pub fn scalar_f64(node: &Value) -> f64 {
    match node {
        Value::Number(number) => number.as_f64().unwrap_or(0.0),
        Value::String(text) => text.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

pub fn scalar_i64(node: &Value) -> i64 {
    match node {
        Value::Number(number) => number.as_i64().or_else(|| number.as_f64().map(|value| value as i64)).unwrap_or(0),
        Value::String(text) => text.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

/// `BigDecimalTypeSerializer.deserialize`.
pub fn scalar_decimal(node: &Value) -> Option<BigDecimal> {
    match node {
        Value::Number(number) => match number.as_i64() {
            Some(value) => Some(BigDecimal::from_i64(value)),
            None => number.as_f64().map(BigDecimal::from_f64),
        },
        Value::String(text) => BigDecimal::parse_128(text),
        _ => None,
    }
}

/// A map's entries, as `ConfigurateUtil.getMap`: keys as text, in file order.
pub fn entries(node: Option<&Value>) -> Vec<(String, Value)> {
    node.and_then(Value::as_mapping).into_iter().flatten().filter_map(|(key, value)| Some((key_text(key)?, value.clone()))).collect()
}

/// A float as SnakeYAML writes it: `f32` keeps its own short digits.
pub fn float_value(value: f32) -> Value {
    Value::Number(Number::from(value.to_string().parse::<f64>().unwrap_or(f64::from(value))))
}

pub struct Config {
    file: PathBuf,
    /// The file made from a resource if this is missing: `templateName`.
    template: Option<&'static str>,
    root: Value,
    transaction: bool,
}

impl Config {
    pub fn new(file: PathBuf) -> Self {
        Self { file, template: None, root: Value::Mapping(Mapping::new()), transaction: false }
    }
    pub fn with_template(file: PathBuf, template: &'static str) -> Self {
        Self { file, template: Some(template), root: Value::Mapping(Mapping::new()), transaction: false }
    }
    pub fn get_file(&self) -> &Path {
        &self.file
    }
    /// `load`.
    pub fn load(&mut self) {
        if PENDING.load(Ordering::SeqCst) != 0 {
            flush();
        }
        if let Some(parent) = self.file.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
            && std::fs::create_dir_all(parent).is_err()
        {
            crate::severe(&crate::adventure::mini_to_legacy(&crate::i18n::tl_literal("failedToCreateConfig", crate::args![self.file.display().to_string()])));
            return;
        }
        if !self.file.exists()
            && let Some(template) = self.template
        {
            crate::info(&crate::adventure::mini_to_legacy(&crate::i18n::tl_literal("creatingConfigFromTemplate", crate::args![self.file.display().to_string()])));
            if let Err(error) = std::fs::write(&self.file, template) {
                crate::severe(&format!("{}: {error}", crate::adventure::mini_to_legacy(&crate::i18n::tl_literal("failedToWriteConfig", crate::args![self.file.display().to_string()]))));
            }
        }
        self.root = match std::fs::read_to_string(&self.file) {
            Err(_) => Value::Mapping(Mapping::new()),
            Ok(text) if text.trim().is_empty() => Value::Mapping(Mapping::new()),
            Ok(text) => match serde_yaml::from_str::<Value>(&text) {
                Ok(Value::Null) => Value::Mapping(Mapping::new()),
                Ok(root) => root,
                Err(error) => {
                    let broken = PathBuf::from(format!("{}.broken.{}", self.file.display(), crate::now_ms()));
                    if std::fs::rename(&self.file, &broken).is_ok() {
                        crate::severe(&format!("The file {} is broken, it has been renamed to {}: {error}", self.file.display(), broken.display()));
                    } else {
                        crate::severe(&format!("The file {} is broken. A backup file has failed to be created: {error}", self.file.display()));
                    }
                    Value::Mapping(Mapping::new())
                }
            },
        };
    }
    /// `getRootNode`.
    pub fn root(&self) -> &Value {
        &self.root
    }
    pub fn root_mut(&mut self) -> &mut Value {
        &mut self.root
    }
    /// `getSection`, and `getInternal`: `None` if it isn't there.
    pub fn get_section(&self, path: &str) -> Option<&Value> {
        node_at(&self.root, path)
    }
    pub fn has_property(&self, path: &str) -> bool {
        self.get_section(path).is_some()
    }
    pub fn get_string(&self, path: &str, default: Option<&str>) -> Option<String> {
        match self.get_section(path) {
            None => default.map(str::to_owned),
            Some(node) => scalar_string(node),
        }
    }
    /// `getString` where the default is text: a section read as text is empty rather than null.
    pub fn string_or(&self, path: &str, default: &str) -> String {
        self.get_string(path, Some(default)).unwrap_or_default()
    }
    pub fn get_boolean(&self, path: &str, default: bool) -> bool {
        self.get_section(path).map_or(default, scalar_bool)
    }
    pub fn is_boolean(&self, path: &str) -> bool {
        self.get_section(path).is_some_and(Value::is_bool)
    }
    pub fn get_long(&self, path: &str, default: i64) -> i64 {
        self.get_section(path).map_or(default, scalar_i64)
    }
    pub fn get_int(&self, path: &str, default: i32) -> i32 {
        self.get_section(path).map_or(default, |node| scalar_i64(node) as i32)
    }
    pub fn get_double(&self, path: &str, default: f64) -> f64 {
        self.get_section(path).map_or(default, scalar_f64)
    }
    pub fn get_big_decimal(&self, path: &str, default: BigDecimal) -> BigDecimal {
        match self.get_section(path) {
            None => default,
            Some(node) => scalar_decimal(node).unwrap_or(default),
        }
    }
    pub fn is_list(&self, path: &str) -> bool {
        self.get_section(path).is_some_and(Value::is_sequence)
    }
    /// `getList(path, String.class)`: a lone value counts as a list of one, as Configurate has it.
    pub fn get_string_list(&self, path: &str) -> Vec<String> {
        match self.get_section(path) {
            None => Vec::new(),
            Some(Value::Sequence(values)) => values.iter().filter_map(scalar_string).collect(),
            Some(node) => scalar_string(node).into_iter().collect(),
        }
    }
    /// `getKeys`: the root's.
    pub fn get_keys(&self) -> Vec<String> {
        entries(Some(&self.root)).into_iter().map(|(key, _)| key).collect()
    }
    pub fn section_keys(&self, path: &str) -> Option<Vec<String>> {
        self.get_section(path).filter(|node| node.is_mapping()).map(|node| entries(Some(node)).into_iter().map(|(key, _)| key).collect())
    }
    pub fn section_map(&self, path: &str) -> Vec<(String, Value)> {
        entries(self.get_section(path))
    }
    /// `setRaw`, and `setProperty`.
    pub fn set(&mut self, path: &str, value: Value) {
        let mut node = &mut self.root;
        for key in split(path) {
            node = child_mut(node, key);
        }
        *node = value;
    }
    /// `removeProperty`.
    pub fn remove(&mut self, path: &str) {
        let keys: Vec<&str> = split(path).collect();
        let Some((last, parents)) = keys.split_last() else { return };
        let mut node = &mut self.root;
        for key in parents {
            if child(node, key).is_none() {
                return;
            }
            node = child_mut(node, key);
        }
        if let Value::Mapping(map) = node {
            let found = map.keys().find(|name| key_text(name).as_deref() == Some(*last)).cloned();
            if let Some(found) = found {
                map.shift_remove(found);
            }
        }
    }
    /// `setProperty(path, location)`: `None` for the root, as the warp files keep it.
    pub fn set_location(&mut self, path: Option<&str>, location: &LazyLocation) {
        match path {
            None => location.write_to(&mut self.root),
            Some(path) => {
                let mut node = Value::Mapping(Mapping::new());
                location.write_to(&mut node);
                self.set(path, node);
            }
        }
    }
    /// `getLocation`.
    pub fn get_location(&self, path: Option<&str>) -> Option<LazyLocation> {
        let node = match path {
            None => Some(&self.root),
            Some(path) => self.get_section(path),
        };
        LazyLocation::read(node?)
    }
    /// `startTransaction`.
    pub fn start_transaction(&mut self) {
        self.transaction = true;
    }
    /// `stopTransaction`.
    pub fn stop_transaction(&mut self, blocking: bool) {
        self.transaction = false;
        if blocking { self.blocking_save() } else { self.save() }
    }
    pub fn is_transaction(&self) -> bool {
        self.transaction
    }
    fn write(&self) {
        let text = serde_yaml::to_string(&self.root).unwrap_or_default();
        // An empty tree is `{}` in serde's form: SnakeYAML's block style writes nothing.
        let text = if text.trim() == "{}" { String::new() } else { text };
        PENDING.fetch_add(1, Ordering::SeqCst);
        if writer().send(Job::Write(self.file.clone(), text.clone())).is_err() {
            write_atomic(&self.file, &text);
            PENDING.fetch_sub(1, Ordering::SeqCst);
        }
    }
    /// `save`.
    pub fn save(&mut self) {
        if !self.transaction {
            self.write();
        }
    }
    /// `blockingSave`.
    pub fn blocking_save(&mut self) {
        self.write();
        flush();
    }
}

/// A `Config` shared by whoever holds it.
pub type SharedConfig = Arc<Mutex<Config>>;

#[cfg(test)]
mod tests {
    use super::*;

    fn config(text: &str) -> Config {
        let mut config = Config::new(PathBuf::from("unused.yml"));
        config.root = serde_yaml::from_str(text).unwrap();
        config
    }

    #[test]
    fn a_node_of_the_wrong_kind_is_zero_not_the_default() {
        let config = config("a:\n  b: text\nc: 12\nd: 'yes'\n");
        assert_eq!(config.get_int("a.b", 5), 0);
        assert_eq!(config.get_int("a.missing", 5), 5);
        assert_eq!(config.get_int("c", 5), 12);
        assert!(config.get_boolean("d", false));
        assert_eq!(config.get_string("c", None).as_deref(), Some("12"));
    }

    #[test]
    fn set_and_remove_walk_dotted_paths() {
        let mut config = config("a: 1\n");
        config.set("x.y.z", Value::from(3));
        assert_eq!(config.get_long("x.y.z", 0), 3);
        config.remove("x.y.z");
        assert!(!config.has_property("x.y.z"));
    }

    #[test]
    fn numeric_keys_answer_to_their_text() {
        let config = config("worth:\n  wool:\n    0: 2.5\n");
        assert_eq!(config.get_double("worth.wool.0", 0.0), 2.5);
    }

    #[test]
    fn floats_keep_their_short_digits() {
        assert_eq!(serde_yaml::to_string(&float_value(-179.29395)).unwrap().trim(), "-179.29395");
    }

    #[test]
    fn the_original_config_parses() {
        let root: Value = serde_yaml::from_str(include_str!("../resources/config.yml")).unwrap();
        assert!(node_at(&root, "teleport-cooldown").is_some());
    }
}
