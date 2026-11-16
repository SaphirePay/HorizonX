//! Bukkit `ConfigurationSection` reads over serde_yaml values: dotted paths, `getString` of any scalar,
//! `getStringList` of a list (or nothing), `getInt` of numbers and numeric text.

use serde_yaml::Value as Yaml;

pub fn at<'a>(v: &'a Yaml, path: &str) -> Option<&'a Yaml> {
    let mut cur = v;
    for key in path.split('.') {
        cur = match cur {
            Yaml::Mapping(m) => m.iter().find(|(k, _)| key_str(k).as_deref() == Some(key)).map(|(_, v)| v)?,
            _ => return None,
        };
    }
    Some(cur)
}

pub fn key_str(k: &Yaml) -> Option<String> {
    match k {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Number(n) => Some(n.to_string()),
        Yaml::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

pub fn contains(v: &Yaml, path: &str) -> bool {
    at(v, path).is_some_and(|v| !v.is_null())
}

pub fn scalar(v: &Yaml) -> Option<String> {
    match v {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Number(n) => Some(n.to_string()),
        Yaml::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

pub fn string(v: &Yaml, path: &str) -> Option<String> {
    at(v, path).and_then(scalar)
}

pub fn is_string(v: &Yaml, path: &str) -> bool {
    matches!(at(v, path), Some(Yaml::String(_)))
}

pub fn is_list(v: &Yaml, path: &str) -> bool {
    matches!(at(v, path), Some(Yaml::Sequence(_)))
}

pub fn is_section(v: &Yaml, path: &str) -> bool {
    matches!(at(v, path), Some(Yaml::Mapping(_)))
}

pub fn is_int(v: &Yaml, path: &str) -> bool {
    matches!(at(v, path), Some(Yaml::Number(n)) if n.is_i64())
}

/// `getStringList`: the list's scalars as text; empty for anything else.
pub fn list(v: &Yaml, path: &str) -> Vec<String> {
    match at(v, path) {
        Some(Yaml::Sequence(items)) => items.iter().filter_map(scalar).collect(),
        _ => Vec::new(),
    }
}

/// A list, or a single string as a one-line list.
pub fn list_or_one(v: &Yaml, path: &str) -> Vec<String> {
    match at(v, path) {
        Some(Yaml::Sequence(items)) => items.iter().filter_map(scalar).collect(),
        Some(other) => scalar(other).into_iter().collect(),
        None => Vec::new(),
    }
}

pub fn int(v: &Yaml, path: &str, default: i64) -> i64 {
    at(v, path)
        .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)).or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
        .unwrap_or(default)
}

pub fn num(v: &Yaml, path: &str, default: f64) -> f64 {
    at(v, path).and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))).unwrap_or(default)
}

pub fn boolean(v: &Yaml, path: &str, default: bool) -> bool {
    match at(v, path) {
        Some(Yaml::Bool(b)) => *b,
        Some(Yaml::String(s)) => s.eq_ignore_ascii_case("true"),
        _ => default,
    }
}

pub fn keys(v: &Yaml, path: &str) -> Vec<String> {
    let section = if path.is_empty() { Some(v) } else { at(v, path) };
    match section {
        Some(Yaml::Mapping(m)) => m.keys().filter_map(key_str).collect(),
        _ => Vec::new(),
    }
}

/// YAML as SnakeYAML (Bukkit) reads it, where serde_yaml is stricter: integers too big for 64 bits
/// (`1000000000000000000000`, zAuctionHouse's multipliers) are read as decimals instead of failing the whole
/// file. Use this (or `Config`, which does) for every plugin file.
pub fn parse(text: &str) -> Result<Yaml, String> {
    // A byte-order mark (Windows editors write one) is no YAML, but SnakeYAML reads past it.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    match serde_yaml::from_str::<Yaml>(text) {
        Ok(v) => Ok(v),
        Err(first) => {
            let fixed = widen_big_integers(text);
            if fixed == text {
                return Err(first.to_string());
            }
            serde_yaml::from_str::<Yaml>(&fixed).map_err(|e| e.to_string())
        }
    }
}

/// Reads a YAML file (`Null` when missing or broken: the caller's defaults apply).
pub fn read(path: &std::path::Path) -> Yaml {
    std::fs::read_to_string(path).ok().and_then(|t| parse(&t).ok()).unwrap_or(Yaml::Null)
}

fn widen_big_integers(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    for line in text.split_inclusive('\n') {
        // The value after `key:` or `- `, up to a comment.
        let value_start = line.find(": ").map(|i| i + 2).or_else(|| line.trim_start().strip_prefix("- ").map(|_| line.find("- ").unwrap_or(0) + 2));
        if let Some(start) = value_start {
            let rest = &line[start..];
            let end = rest.find(" #").unwrap_or(rest.trim_end_matches(['\r', '\n']).len());
            let token = rest[..end].trim();
            let digits = token.strip_prefix('-').unwrap_or(token);
            if digits.len() > 18 && digits.chars().all(|c| c.is_ascii_digit()) {
                let at = start + rest.find(token).unwrap_or(0);
                out.push_str(&line[..at + token.len()]);
                out.push_str(".0");
                out.push_str(&line[at + token.len()..]);
                continue;
            }
        }
        out.push_str(line);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn big_integers_read() {
        let v = super::parse("a:\n  m: 1000000000000000000000\n  n: 5\nb:\n  - 99999999999999999999 # x\n").unwrap();
        assert_eq!(super::num(&v, "a.m", 0.0), 1e21);
        assert_eq!(super::int(&v, "a.n", 0), 5);
        assert!(v["b"][0].as_f64().is_some());
    }
}
