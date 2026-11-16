//! SnakeYAML's `Yaml`, which the server ships and plugins use without shading: `load` gives Java's maps,
//! lists and scalars, `dump` writes block YAML. The options objects only carry settings, so they are
//! stand-ins that accept them all.
use horizon_bukkit::config::{self, YamlConfiguration};
use jrt::{R, V};

pub struct YamlH;

pub fn construct(class: &str, _a: &[V]) -> Option<R> {
    let simple = class.rsplit('.').next().unwrap_or(class);
    Some(Ok(match simple {
        "Yaml" => jrt::native(YamlH),
        // SnakeYAML's `UnicodeReader(stream)`: the stream, read as text (UTF-8, its byte-order mark dropped by the parser).
        "UnicodeReader" => _a.first().cloned().unwrap_or(V::Null),
        // Bukkit's own `YamlConstructor`/`YamlRepresenter` (what `YamlConfiguration` hands SnakeYAML) too.
        "DumperOptions" | "LoaderOptions" | "Representer" | "Constructor" | "SafeConstructor" | "Resolver" | "YamlConstructor" | "YamlRepresenter" | "SafeRepresenter" | "BaseConstructor" => jrt::new_builtin("JStub", &[])?.ok()?,
        _ => return None,
    }))
}

fn text_of(value: &V) -> String {
    match value {
        V::Str(text) => text.to_string(),
        other => match jrt::javaio::bytes_of(other) {
            Some(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            None => read_all(other).unwrap_or_else(|| other.text()),
        },
    }
}

/// Everything a `Reader` (the plugin's own, as WorldEdit's `UnicodeReader`) or an `InputStream` still has.
fn read_all(reader: &V) -> Option<String> {
    let buffer = jrt::new_array("char", &[V::Int(8192)]);
    let mut out = String::new();
    for _ in 0..1_000_000 {
        let count = jrt::call(reader, "read", &[buffer.clone(), V::Int(0), V::Int(8192)]).ok()?.int();
        if count < 0 {
            return Some(out);
        }
        let items = buffer.items().ok()?;
        for item in items.iter().take(count as usize) {
            match item {
                V::Char(c) => out.push(*c),
                // A byte stream's `read(byte[])`: the bytes, as UTF-8 is read below.
                V::Int(b) => out.push(char::from_u32(*b as u8 as u32).unwrap_or('?')),
                other => out.push_str(&other.text()),
            }
        }
    }
    Some(out)
}

/// `DumperOptions.LineBreak`: the line ending, by `getString()`.
pub struct LineBreakH(pub &'static str);

fn dump(value: &V) -> String {
    let json = jrt::javaio::to_json(value);
    match json {
        serde_json::Value::Object(map) => {
            let mut out = YamlConfiguration::new();
            out.set_path_separator('\u{0}');
            for (key, item) in map {
                out.set(&key, item);
            }
            out.save_to_string()
        }
        serde_json::Value::Null => "null\n".to_owned(),
        other => format!("{}\n", config::scalar(&other).unwrap_or_else(|| other.to_string())),
    }
}

/// `LineBreak.getPlatformLineBreak()` and its constants: Unix's, as the server's files are written on Linux.
pub fn static_call(class: &str, name: &str) -> Option<R> {
    match (class.rsplit(['.', '$']).next().unwrap_or(class), name) {
        ("LineBreak", "getPlatformLineBreak") => Some(Ok(jrt::native(LineBreakH("
")))),
        _ => None,
    }
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if let Some(line_break) = this.native::<LineBreakH>() {
        return match name {
            "getString" => Some(Ok(jrt::s(line_break.0))),
            "toString" | "name" => Some(Ok(jrt::s(if line_break.0 == "
" { "UNIX" } else { "WIN" }))),
            _ => None,
        };
    }
    this.native::<YamlH>()?;
    Some(Ok(match (name, a.len()) {
        ("load" | "loadAs" | "compose", 1..) => {
            let parsed = config::parse(&text_of(&a[0]));
            jrt::javaio::from_json(&parsed)
        }
        ("loadAll", 1) => jrt::list(vec![jrt::javaio::from_json(&config::parse(&text_of(&a[0])))]),
        ("dump" | "dumpAsMap", 1) => jrt::s(&dump(&a[0])),
        ("dump" | "dumpAsMap", 2) => {
            if let Err(error) = jrt::call(&a[1], "write", &[jrt::s(&dump(&a[0]))]) {
                return Some(Err(error));
            }
            V::Null
        }
        ("hashCode", 0) => V::Int(0),
        ("toString", 0) => jrt::s("Yaml"),
        _ => return None,
    }))
}
