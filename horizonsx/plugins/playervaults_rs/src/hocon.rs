//! The HOCON that PlayerVaultsX's config.conf and lang.conf are written in (typesafe-config's output): objects
//! with `key=value`, `key: value` or `key { .. }`, lists `[..]`, quoted or bare strings, `#` and `//` comments,
//! dotted keys, `"""triple quoted"""` strings. Read into JSON values.

use serde_json::{Map, Value};

struct P<'a> {
    s: &'a [char],
    i: usize,
}

impl P<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }
    fn starts(&self, t: &str) -> bool {
        t.chars().enumerate().all(|(k, c)| self.s.get(self.i + k) == Some(&c))
    }
    /// Spaces, new lines (when `lines`), commas (when `commas`) and comments.
    fn skip(&mut self, lines: bool, commas: bool) {
        while let Some(c) = self.peek() {
            if c == '#' || self.starts("//") {
                while self.peek().is_some_and(|c| c != '\n') {
                    self.i += 1;
                }
            } else if c == ' ' || c == '\t' || c == '\r' || c == '\u{feff}' || (lines && c == '\n') || (commas && c == ',') {
                self.i += 1;
            } else {
                break;
            }
        }
    }
    fn quoted(&mut self) -> String {
        if self.starts("\"\"\"") {
            self.i += 3;
            let mut out = String::new();
            while self.i < self.s.len() && !self.starts("\"\"\"") {
                out.push(self.s[self.i]);
                self.i += 1;
            }
            self.i += 3;
            return out;
        }
        self.i += 1;
        let mut out = String::new();
        while let Some(c) = self.peek() {
            self.i += 1;
            match c {
                '"' => break,
                '\\' => {
                    let e = self.peek().unwrap_or('\\');
                    self.i += 1;
                    match e {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'u' => {
                            let hex: String = self.s[self.i..(self.i + 4).min(self.s.len())].iter().collect();
                            self.i += 4;
                            if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                                out.push(ch);
                            }
                        }
                        other => out.push(other),
                    }
                }
                other => out.push(other),
            }
        }
        out
    }
    fn key(&mut self) -> Vec<String> {
        let mut parts = Vec::new();
        loop {
            if self.peek() == Some('"') {
                parts.push(self.quoted());
            } else {
                let mut k = String::new();
                while let Some(c) = self.peek() {
                    if c.is_whitespace() || matches!(c, '=' | ':' | '{' | '.' | '"' | '[' | ',' | '}' | '#') {
                        break;
                    }
                    k.push(c);
                    self.i += 1;
                }
                parts.push(k);
            }
            if self.peek() == Some('.') {
                self.i += 1;
                continue;
            }
            break;
        }
        parts
    }
    fn value(&mut self) -> Value {
        self.skip(false, false);
        match self.peek() {
            Some('{') => {
                self.i += 1;
                let v = self.object(true);
                if self.peek() == Some('}') {
                    self.i += 1;
                }
                v
            }
            Some('[') => {
                self.i += 1;
                let mut items = Vec::new();
                loop {
                    self.skip(true, true);
                    match self.peek() {
                        Some(']') => {
                            self.i += 1;
                            break;
                        }
                        None => break,
                        _ => items.push(self.value()),
                    }
                }
                Value::Array(items)
            }
            Some('"') => {
                let s = self.quoted();
                // Concatenation with what follows on the line ("a" "b").
                let mut out = s;
                loop {
                    let save = self.i;
                    while self.peek().is_some_and(|c| c == ' ' || c == '\t') {
                        self.i += 1;
                    }
                    if self.peek() == Some('"') {
                        out.push_str(&self.quoted());
                    } else {
                        self.i = save;
                        break;
                    }
                }
                Value::String(out)
            }
            _ => {
                let mut raw = String::new();
                while let Some(c) = self.peek() {
                    if matches!(c, '\n' | ',' | '}' | ']' | '#') || self.starts("//") {
                        break;
                    }
                    raw.push(c);
                    self.i += 1;
                }
                let t = raw.trim();
                match t {
                    "true" | "yes" | "on" => Value::Bool(true),
                    "false" | "no" | "off" => Value::Bool(false),
                    "null" => Value::Null,
                    _ => t.parse::<i64>().map(Value::from).or_else(|_| t.parse::<f64>().map(Value::from)).unwrap_or_else(|_| Value::String(t.to_owned())),
                }
            }
        }
    }
    fn object(&mut self, braced: bool) -> Value {
        let mut map = Map::new();
        loop {
            self.skip(true, true);
            match self.peek() {
                None => break,
                Some('}') if braced => break,
                Some('}') => {
                    self.i += 1;
                    continue;
                }
                _ => {}
            }
            let path = self.key();
            if path.iter().all(String::is_empty) {
                self.i += 1;
                continue;
            }
            self.skip(false, false);
            let value = match self.peek() {
                Some('=' | ':') => {
                    self.i += 1;
                    self.value()
                }
                Some('{') => self.value(),
                _ => self.value(),
            };
            insert(&mut map, &path, value);
        }
        Value::Object(map)
    }
}

fn insert(map: &mut Map<String, Value>, path: &[String], value: Value) {
    let Some((first, rest)) = path.split_first() else { return };
    if rest.is_empty() {
        match (map.get_mut(first), value) {
            (Some(Value::Object(old)), Value::Object(new)) => {
                for (k, v) in new {
                    insert(old, std::slice::from_ref(&k), v);
                }
            }
            (_, v) => {
                map.insert(first.clone(), v);
            }
        }
        return;
    }
    let entry = map.entry(first.clone()).or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    if let Value::Object(inner) = entry {
        insert(inner, rest, value);
    }
}

pub fn parse(text: &str) -> Value {
    let chars: Vec<char> = text.chars().collect();
    let mut p = P { s: &chars, i: 0 };
    p.skip(true, false);
    if p.peek() == Some('{') {
        p.i += 1;
    }
    p.object(false)
}

/// A JSON string as HOCON writes it.
pub fn quote(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads() {
        let v = super::parse("# hi\na=1\nb {\n  c=\"x y\"\n  d=[\n    PUMPKIN,\n    \"A\"\n  ]\n}\ne.f=true\n");
        assert_eq!(v["a"], 1);
        assert_eq!(v["b"]["c"], "x y");
        assert_eq!(v["b"]["d"][0], "PUMPKIN");
        assert_eq!(v["e"]["f"], true);
    }
}
