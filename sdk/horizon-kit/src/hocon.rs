//! HOCON, as Configurate writes it (jpenilla's plugins: MiniMOTD, AnnouncerPlus, TabTPS...): objects with or without
//! the outer braces, `=`/`:`/nothing before `{`, comments (`#`, `//`), quoted, triple-quoted and unquoted strings,
//! numbers, booleans, null, arrays, dotted keys, commas or newlines between entries, duplicate objects merged.
//! Read into `serde_json::Value`; [`write`] gives the text back (for saving a changed or default file).

use serde_json::{Map, Number, Value};

struct P<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> P<'a> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    /// Spaces, tabs and comments; newlines too when `nl`.
    fn skip(&mut self, nl: bool) {
        while let Some(c) = self.peek() {
            match c {
                b' ' | b'\t' | b'\r' => self.i += 1,
                b'\n' if nl => self.i += 1,
                b',' if nl => self.i += 1,
                b'#' => self.line(),
                b'/' if self.s.get(self.i + 1) == Some(&b'/') => self.line(),
                _ => break,
            }
        }
    }
    fn line(&mut self) {
        while let Some(c) = self.peek() {
            if c == b'\n' {
                break;
            }
            self.i += 1;
        }
    }
    fn err(&self, what: &str) -> String {
        let line = self.s[..self.i.min(self.s.len())].iter().filter(|c| **c == b'\n').count() + 1;
        format!("{what} at line {line}")
    }

    fn quoted(&mut self) -> Result<String, String> {
        if self.s[self.i..].starts_with(b"\"\"\"") {
            self.i += 3;
            let start = self.i;
            while self.i < self.s.len() && !self.s[self.i..].starts_with(b"\"\"\"") {
                self.i += 1;
            }
            let out = String::from_utf8_lossy(&self.s[start..self.i]).into_owned();
            self.i = (self.i + 3).min(self.s.len());
            return Ok(out);
        }
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        while let Some(c) = self.peek() {
            self.i += 1;
            match c {
                b'"' => return Ok(String::from_utf8_lossy(&out).into_owned()),
                b'\\' => {
                    let e = self.peek().ok_or_else(|| self.err("unfinished escape"))?;
                    self.i += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'u' => {
                            let hex = std::str::from_utf8(self.s.get(self.i..self.i + 4).unwrap_or_default()).unwrap_or("");
                            let ch = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32).unwrap_or('?');
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                            self.i += 4;
                        }
                        other => out.push(other),
                    }
                }
                _ => out.push(c),
            }
        }
        Err(self.err("unfinished string"))
    }

    fn key(&mut self) -> Result<Vec<String>, String> {
        let mut parts = Vec::new();
        loop {
            if self.peek() == Some(b'"') {
                parts.push(self.quoted()?);
            } else {
                let start = self.i;
                while let Some(c) = self.peek() {
                    if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_') || c >= 0x80 {
                        self.i += 1;
                    } else {
                        break;
                    }
                }
                if start == self.i {
                    return Err(self.err("expected a key"));
                }
                parts.push(String::from_utf8_lossy(&self.s[start..self.i]).into_owned());
            }
            if self.peek() == Some(b'.') {
                self.i += 1;
                continue;
            }
            return Ok(parts);
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        self.skip(false);
        match self.peek() {
            Some(b'{') => {
                self.i += 1;
                let v = self.object(true)?;
                Ok(Value::Object(v))
            }
            Some(b'[') => {
                self.i += 1;
                let mut arr = Vec::new();
                loop {
                    self.skip(true);
                    match self.peek() {
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Value::Array(arr));
                        }
                        None => return Err(self.err("unfinished list")),
                        _ => arr.push(self.value()?),
                    }
                }
            }
            Some(b'"') => Ok(Value::String(self.quoted()?)),
            _ => {
                let start = self.i;
                while let Some(c) = self.peek() {
                    if matches!(c, b'\n' | b',' | b'}' | b']' | b'#') || (c == b'/' && self.s.get(self.i + 1) == Some(&b'/')) {
                        break;
                    }
                    self.i += 1;
                }
                let raw = String::from_utf8_lossy(&self.s[start..self.i]).trim().to_owned();
                Ok(match raw.as_str() {
                    "true" | "yes" | "on" => Value::Bool(true),
                    "false" | "no" | "off" => Value::Bool(false),
                    "null" => Value::Null,
                    _ => raw
                        .parse::<i64>()
                        .ok()
                        .map(|n| Value::Number(n.into()))
                        .or_else(|| raw.parse::<f64>().ok().and_then(Number::from_f64).map(Value::Number))
                        .unwrap_or(Value::String(raw)),
                })
            }
        }
    }

    fn object(&mut self, braced: bool) -> Result<Map<String, Value>, String> {
        let mut map = Map::new();
        loop {
            self.skip(true);
            match self.peek() {
                None if !braced => return Ok(map),
                None => return Err(self.err("unfinished object")),
                Some(b'}') if braced => {
                    self.i += 1;
                    return Ok(map);
                }
                _ => {}
            }
            let key = self.key()?;
            self.skip(false);
            let v = match self.peek() {
                Some(b'=') | Some(b':') => {
                    self.i += 1;
                    self.value()?
                }
                Some(b'{') => self.value()?,
                Some(b'+') if self.s.get(self.i + 1) == Some(&b'=') => {
                    self.i += 2;
                    let v = self.value()?;
                    Value::Array(vec![v])
                }
                _ => return Err(self.err("expected = after a key")),
            };
            insert(&mut map, &key, v);
        }
    }
}

fn insert(map: &mut Map<String, Value>, key: &[String], v: Value) {
    if key.len() == 1 {
        match (map.get_mut(&key[0]), v) {
            (Some(Value::Object(old)), Value::Object(new)) => {
                for (k, val) in new {
                    insert(old, &[k], val);
                }
            }
            (_, v) => {
                map.insert(key[0].clone(), v);
            }
        }
        return;
    }
    let entry = map.entry(key[0].clone()).or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    if let Value::Object(inner) = entry {
        insert(inner, &key[1..], v);
    }
}

/// HOCON text -> a JSON object.
pub fn parse(text: &str) -> Result<Value, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut p = P { s: text.as_bytes(), i: 0 };
    p.skip(true);
    if p.peek() == Some(b'{') {
        p.i += 1;
        return p.object(true).map(Value::Object);
    }
    p.object(false).map(Value::Object)
}

/// Reads a file; missing or broken = Err with the reason.
pub fn read(path: &std::path::Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn key_text(k: &str) -> String {
    if !k.is_empty() && k.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') { k.to_owned() } else { quote(k) }
}

fn write_value(v: &Value, indent: usize, out: &mut String) {
    let pad = "    ".repeat(indent);
    match v {
        Value::Object(m) => {
            out.push_str("{\n");
            write_entries(m, indent + 1, out);
            out.push_str(&pad);
            out.push('}');
        }
        Value::Array(a) => {
            if a.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push_str("[\n");
            for item in a {
                out.push_str(&"    ".repeat(indent + 1));
                write_value(item, indent + 1, out);
                out.push('\n');
            }
            out.push_str(&pad);
            out.push(']');
        }
        Value::String(s) => out.push_str(&quote(s)),
        Value::Null => out.push_str("null"),
        other => out.push_str(&other.to_string()),
    }
}

fn write_entries(m: &Map<String, Value>, indent: usize, out: &mut String) {
    for (k, v) in m {
        out.push_str(&"    ".repeat(indent));
        out.push_str(&key_text(k));
        out.push_str(if v.is_object() { " " } else { "=" });
        write_value(v, indent, out);
        out.push('\n');
    }
}

/// A JSON object as HOCON text (no outer braces, Configurate's layout), with `header` as `#` lines on top.
pub fn write(v: &Value, header: &str) -> String {
    let mut out = String::new();
    for line in header.lines() {
        out.push_str(&format!("# {line}\n"));
    }
    if let Value::Object(m) = v {
        write_entries(m, 0, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_configurate_output() {
        let t = r#"
# header
motd-enabled=true
// other comment
motds=[
    {
        icon=random
        line1="<rainbow>MiniMOTD Default"
        line2="MiniMessage <gradient:blue:red>Gradients"
    }
]
player-count-settings {
    fake-players {
        fake-players="25%"
        fake-players-enabled=false
    }
    max-players=69
    servers=[]
}
a.b.c = 1.5, d: "x"
"#;
        let v = parse(t).unwrap();
        assert_eq!(v["motd-enabled"], true);
        assert_eq!(v["motds"][0]["icon"], "random");
        assert_eq!(v["motds"][0]["line2"], "MiniMessage <gradient:blue:red>Gradients");
        assert_eq!(v["player-count-settings"]["max-players"], 69);
        assert_eq!(v["player-count-settings"]["fake-players"]["fake-players"], "25%");
        assert_eq!(v["a"]["b"]["c"], 1.5);
        assert_eq!(v["d"], "x");
        let again = parse(&write(&v, "h")).unwrap();
        assert_eq!(again, v);
    }
}
