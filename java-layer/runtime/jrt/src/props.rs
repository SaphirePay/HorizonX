//! `java.util.Properties`: a map of text to text, read from and written as `key=value` lines.
use crate::{JMap, R, V, s};
use std::sync::{Arc, Mutex};

pub fn construct(class: &str, a: &[V]) -> Option<R> {
    (class == "Properties").then(|| {
        let mut map = JMap::new();
        // `new Properties(defaults)`: its entries, copied.
        if let Some(V::Map(defaults)) = a.first() {
            for (key, pair) in crate::lock(defaults).iter() {
                map.insert(key.clone(), pair.clone());
            }
        }
        Ok(V::Map(Arc::new(Mutex::new(map))))
    })
}

fn unescape(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('f') => out.push('\u{c}'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                out.extend(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32));
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

fn escape(text: &str, key: bool) -> String {
    let mut out = String::new();
    for (i, c) in text.chars().enumerate() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '=' | ':' | '#' | '!' if key => {
                out.push('\\');
                out.push(c);
            }
            ' ' if key || i == 0 => out.push_str("\\ "),
            c if (c as u32) > 0x7e => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// The `key=value` pairs of a properties text, continuation lines joined.
pub fn parse(text: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut logical = String::new();
    for line in text.lines() {
        let line = if logical.is_empty() { line.trim_start() } else { line.trim_start() };
        if logical.is_empty() && (line.is_empty() || line.starts_with('#') || line.starts_with('!')) {
            continue;
        }
        let trailing = line.chars().rev().take_while(|c| *c == '\\').count();
        if trailing % 2 == 1 {
            logical.push_str(&line[..line.len() - 1]);
            continue;
        }
        logical.push_str(line);
        let chars: Vec<char> = logical.chars().collect();
        let mut at = 0;
        while at < chars.len() {
            match chars[at] {
                '\\' => at += 2,
                '=' | ':' | ' ' | '\t' => break,
                _ => at += 1,
            }
        }
        let key: String = chars[..at.min(chars.len())].iter().collect();
        let mut rest = at;
        while rest < chars.len() && matches!(chars[rest], ' ' | '\t') {
            rest += 1;
        }
        if rest < chars.len() && matches!(chars[rest], '=' | ':') {
            rest += 1;
        }
        while rest < chars.len() && matches!(chars[rest], ' ' | '\t') {
            rest += 1;
        }
        let value: String = chars[rest.min(chars.len())..].iter().collect();
        pairs.push((unescape(&key), unescape(&value)));
        logical.clear();
    }
    pairs
}

fn source_text(value: &V) -> String {
    match value {
        V::Str(text) => text.to_string(),
        other => crate::javaio::bytes_of(other).map_or_else(String::new, |bytes| String::from_utf8_lossy(&bytes).into_owned()),
    }
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    let V::Map(map) = this else { return None };
    Some(Ok(match (name, a.len()) {
        ("getProperty", 1 | 2) => {
            let found = crate::lock(map).get(&a[0].key()).map(|pair| pair.1.clone());
            match found {
                Some(value) if !value.is_null() => value,
                _ => a.get(1).cloned().unwrap_or(V::Null),
            }
        }
        ("setProperty", 2) => {
            let old = crate::lock(map).insert(a[0].key(), (a[0].clone(), a[1].clone()));
            old.map_or(V::Null, |pair| pair.1)
        }
        ("load", 1) => {
            let text = source_text(&a[0]);
            let mut map = crate::lock(map);
            for (key, value) in parse(&text) {
                let key = s(&key);
                map.insert(key.key(), (key, s(&value)));
            }
            V::Null
        }
        ("store", 2) => {
            let mut text = String::new();
            if !a[1].is_null() {
                text.push_str(&format!("#{}\n", a[1].text()));
            }
            for (key, value) in crate::lock(map).values() {
                text.push_str(&format!("{}={}\n", escape(&key.text(), true), escape(&value.text(), false)));
            }
            if let Err(error) = crate::call(&a[0], "write", &[s(&text)]) {
                return Some(Err(error));
            }
            let _ = crate::call(&a[0], "flush", &[]);
            V::Null
        }
        ("stringPropertyNames", 0) => {
            let keys: Vec<V> = crate::lock(map).values().map(|pair| pair.0.clone()).collect();
            crate::call_static("Set", "of", &keys).unwrap_or(V::Null)
        }
        ("propertyNames", 0) => {
            let keys: Vec<V> = crate::lock(map).values().map(|pair| pair.0.clone()).collect();
            return Some(crate::call(&crate::list(keys), "iterator", &[]));
        }
        _ => return None,
    }))
}
