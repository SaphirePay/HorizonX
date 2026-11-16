//! A plugin's YAML files as Java plugins keep them: the default written out on first start (the jar's own file,
//! `include_str!`'d), read back with typed getters by dotted path, and a messages file (`lang.yml`,
//! `messages.yml`) with `{prefix}` and Java `String.format` arguments.

use serde_yaml::Value as Yaml;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One YAML file, read.
#[derive(Clone, Default)]
pub struct Config {
    pub path: PathBuf,
    pub doc: Yaml,
}

impl Config {
    /// `folder/name`, written from `default` first when it isn't there; keys the file lacks are read from
    /// `default` (as Bukkit's `getConfig()` with its defaults).
    pub fn load(folder: &Path, name: &str, default: &str) -> Config {
        let path = folder.join(name);
        if !path.exists() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&path, default);
        }
        let mut doc: Yaml = std::fs::read_to_string(&path).ok().and_then(|t| crate::yaml::parse(&t).ok()).unwrap_or(Yaml::Null);
        if let Ok(defaults) = crate::yaml::parse(default) {
            merge(&mut doc, &defaults);
        }
        Config { path, doc }
    }

    /// From text alone (tests, built-in defaults).
    pub fn parse(text: &str) -> Config {
        Config { path: PathBuf::new(), doc: crate::yaml::parse(text).unwrap_or(Yaml::Null) }
    }

    pub fn at(&self, path: &str) -> Option<&Yaml> {
        let mut v = &self.doc;
        for key in path.split('.') {
            v = v.get(key)?;
        }
        Some(v)
    }
    pub fn str(&self, path: &str, default: &str) -> String {
        match self.at(path) {
            Some(Yaml::String(s)) => s.clone(),
            Some(Yaml::Number(n)) => n.to_string(),
            Some(Yaml::Bool(b)) => b.to_string(),
            _ => default.to_owned(),
        }
    }
    pub fn int(&self, path: &str, default: i64) -> i64 {
        self.at(path).and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))).unwrap_or(default)
    }
    pub fn num(&self, path: &str, default: f64) -> f64 {
        self.at(path).and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))).unwrap_or(default)
    }
    pub fn bool(&self, path: &str, default: bool) -> bool {
        self.at(path).and_then(Yaml::as_bool).unwrap_or(default)
    }
    pub fn list(&self, path: &str) -> Vec<String> {
        match self.at(path) {
            Some(Yaml::Sequence(items)) => items.iter().filter_map(|v| v.as_str().map(str::to_owned).or_else(|| v.as_i64().map(|n| n.to_string()))).collect(),
            Some(Yaml::String(s)) => vec![s.clone()],
            _ => Vec::new(),
        }
    }
    /// The keys under a section (`getConfigurationSection(path).getKeys(false)`).
    pub fn keys(&self, path: &str) -> Vec<String> {
        let section = if path.is_empty() { Some(&self.doc) } else { self.at(path) };
        section.and_then(Yaml::as_mapping).map(|m| m.keys().filter_map(|k| k.as_str().map(str::to_owned)).collect()).unwrap_or_default()
    }
    /// A value set (dotted path, sections made as needed) and the file written.
    pub fn set(&mut self, path: &str, value: Yaml) {
        let mut v = &mut self.doc;
        for key in path.split('.') {
            if !v.is_mapping() {
                *v = Yaml::Mapping(Default::default());
            }
            let map = v.as_mapping_mut().expect("a mapping");
            v = map.entry(Yaml::String(key.to_owned())).or_insert(Yaml::Null);
        }
        *v = value;
    }
    pub fn save(&self) {
        if let Ok(text) = serde_yaml::to_string(&self.doc) {
            let _ = std::fs::write(&self.path, text);
        }
    }
}

fn merge(doc: &mut Yaml, defaults: &Yaml) {
    match (doc, defaults) {
        (Yaml::Mapping(mine), Yaml::Mapping(theirs)) => {
            for (k, v) in theirs {
                match mine.get_mut(k) {
                    Some(existing) => merge(existing, v),
                    None => {
                        mine.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (doc @ Yaml::Null, d) => *doc = d.clone(),
        _ => {}
    }
}

/// Messages by dotted key (sections flattened), with the prefix filled in.
#[derive(Clone, Default)]
pub struct Lang {
    pub messages: HashMap<String, String>,
    pub prefix: String,
}

impl Lang {
    /// From a messages file (see [`Config::load`]); `prefix_key` names the prefix message (often `prefix`).
    pub fn load(folder: &Path, name: &str, default: &str, prefix_key: &str) -> Lang {
        Lang::of(&Config::load(folder, name, default), prefix_key)
    }
    pub fn of(config: &Config, prefix_key: &str) -> Lang {
        let mut messages = HashMap::new();
        if let Some(m) = config.doc.as_mapping() {
            flatten(m, "", &mut messages);
        }
        let prefix = messages.get(prefix_key).cloned().unwrap_or_default();
        Lang { messages, prefix }
    }
    /// The message with `{prefix}`/`%prefix%` and the arguments (`%s`, `%1$s`, `{0}`) filled in; the key itself when
    /// there's no such message.
    pub fn get(&self, key: &str, args: &[&str]) -> String {
        let raw = self.messages.get(key).cloned().unwrap_or_else(|| key.to_owned());
        let text = raw.replace("{prefix}", &self.prefix).replace("%prefix%", &self.prefix);
        crate::fmt::java(&crate::fmt::braces(&text, args), args)
    }
    /// A message that is a list in the file (help pages), each line filled in.
    pub fn lines(&self, key: &str, args: &[&str]) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        while let Some(line) = self.messages.get(&format!("{key}.{i}")) {
            out.push(crate::fmt::java(&crate::fmt::braces(&line.replace("{prefix}", &self.prefix), args), args));
            i += 1;
        }
        if out.is_empty() && self.messages.contains_key(key) {
            out.push(self.get(key, args));
        }
        out
    }
}

fn flatten(m: &serde_yaml::Mapping, prefix: &str, out: &mut HashMap<String, String>) {
    for (k, v) in m {
        let Some(k) = k.as_str().map(str::to_owned).or_else(|| k.as_i64().map(|n| n.to_string())) else { continue };
        let key = if prefix.is_empty() { k } else { format!("{prefix}.{k}") };
        match v {
            Yaml::Mapping(inner) => flatten(inner, &key, out),
            Yaml::String(s) => {
                out.insert(key, s.clone());
            }
            // A list (a help page): each line as key.0, key.1 ..., and all of them joined as the key.
            Yaml::Sequence(items) => {
                let lines: Vec<String> = items.iter().filter_map(|i| i.as_str().map(str::to_owned)).collect();
                for (n, line) in lines.iter().enumerate() {
                    out.insert(format!("{key}.{n}"), line.clone());
                }
                out.insert(key, lines.join("\n"));
            }
            other => {
                if let Some(s) = other.as_i64().map(|n| n.to_string()).or_else(|| other.as_f64().map(|n| n.to_string())).or_else(|| other.as_bool().map(|b| b.to_string())) {
                    out.insert(key, s);
                }
            }
        }
    }
}
