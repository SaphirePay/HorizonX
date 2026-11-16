//! `groups.yml` / `users.yml`: a holder's properties (`tabprefix`, `tagprefix`, `customtabname`, ...), with the
//! `per-world` overrides, in TAB's order: the world's value for the holder, the holder's, then `_DEFAULT_`'s.

use serde_yaml::Value as Yaml;
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct Props {
    root: Yaml,
}

pub fn scalar(v: &Yaml) -> Option<String> {
    match v {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Number(n) => Some(n.to_string()),
        Yaml::Bool(b) => Some(b.to_string()),
        Yaml::Sequence(s) => Some(s.iter().filter_map(scalar).collect::<Vec<_>>().join("\n")),
        _ => None,
    }
}

fn get_ci<'a>(map: &'a Yaml, key: &str) -> Option<&'a Yaml> {
    let m = map.as_mapping()?;
    m.get(key).or_else(|| m.iter().find(|(k, _)| k.as_str().is_some_and(|k| k.eq_ignore_ascii_case(key))).map(|(_, v)| v))
}

impl Props {
    pub fn load(path: &Path, default: &str) -> Result<Props, String> {
        if !path.exists() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, default);
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let root: Yaml = if text.trim().is_empty() { Yaml::Mapping(Default::default()) } else { horizon_kit::yaml::parse(&text).map_err(|_| path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default())? };
        Ok(Props { root })
    }

    fn holder_prop(&self, holder: &str, world: &str, prop: &str) -> Option<String> {
        let from_world = get_ci(&self.root, "per-world").and_then(|pw| {
            pw.as_mapping()?.iter().find_map(|(worlds, holders)| {
                // A key may list several worlds: `world1;world2`.
                let names = worlds.as_str()?;
                names.split(';').any(|w| w.trim() == world).then(|| get_ci(holders, holder).and_then(|h| get_ci(h, prop)).and_then(scalar)).flatten()
            })
        });
        from_world.or_else(|| get_ci(&self.root, holder).and_then(|h| get_ci(h, prop)).and_then(scalar))
    }

    /// A group's property, falling back to `_DEFAULT_`.
    pub fn group(&self, group: &str, world: &str, prop: &str) -> Option<String> {
        self.holder_prop(group, world, prop).or_else(|| self.holder_prop("_DEFAULT_", world, prop))
    }

    /// A player's own property (by name or uuid).
    pub fn user(&self, name: &str, uuid: &str, world: &str, prop: &str) -> Option<String> {
        self.holder_prop(name, world, prop).or_else(|| self.holder_prop(uuid, world, prop))
    }

    /// The configured group spelled as in the file (`owner` -> `Owner`).
    pub fn find_group(&self, name: &str) -> Option<String> {
        self.root.as_mapping()?.keys().filter_map(Yaml::as_str).find(|k| k.eq_ignore_ascii_case(name)).map(str::to_owned)
    }

    pub fn holders(&self) -> Vec<String> {
        self.root.as_mapping().map(|m| m.keys().filter_map(Yaml::as_str).filter(|k| !matches!(*k, "per-world" | "per-server")).map(str::to_owned).collect()).unwrap_or_default()
    }
}

fn read(path: &Path) -> Yaml {
    std::fs::read_to_string(path).ok().and_then(|t| horizon_kit::yaml::parse(&t).ok()).filter(Yaml::is_mapping).unwrap_or_else(|| Yaml::Mapping(Default::default()))
}

fn write(path: &Path, root: &Yaml) {
    if let Ok(text) = serde_yaml::to_string(root) {
        let _ = std::fs::write(path, text);
    }
}

/// `/tab group|player <name> <property> [value] [-w world]`: sets (or with no value removes) one property.
pub fn set_property(path: &Path, holder: &str, prop: &str, value: Option<&str>, world: Option<&str>) {
    let mut root = read(path);
    let Some(top) = root.as_mapping_mut() else { return };
    let target = match world {
        Some(w) => {
            let pw = top.entry(Yaml::from("per-world")).or_insert_with(|| Yaml::Mapping(Default::default()));
            let Some(pw) = pw.as_mapping_mut() else { return };
            let wm = pw.entry(Yaml::from(w)).or_insert_with(|| Yaml::Mapping(Default::default()));
            let Some(wm) = wm.as_mapping_mut() else { return };
            wm.entry(Yaml::from(holder)).or_insert_with(|| Yaml::Mapping(Default::default()))
        }
        None => top.entry(Yaml::from(holder)).or_insert_with(|| Yaml::Mapping(Default::default())),
    };
    if !target.is_mapping() {
        *target = Yaml::Mapping(Default::default());
    }
    if let Some(m) = target.as_mapping_mut() {
        match value {
            Some(v) => {
                m.insert(Yaml::from(prop), Yaml::from(v));
            }
            None => {
                m.remove(prop);
            }
        }
    }
    write(path, &root);
}

/// `/tab group|player <name> remove`.
pub fn remove_holder(path: &Path, holder: &str) {
    let mut root = read(path);
    if let Some(top) = root.as_mapping_mut() {
        top.remove(holder);
        if let Some(pw) = top.get_mut("per-world").and_then(Yaml::as_mapping_mut) {
            for (_, holders) in pw.iter_mut() {
                if let Some(h) = holders.as_mapping_mut() {
                    h.remove(holder);
                }
            }
        }
    }
    write(path, &root);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lookup_order() {
        let p = Props { root: horizon_kit::yaml::parse(include_str!("../data/config/groups.yml")).unwrap() };
        assert_eq!(p.group("Player", "world", "tabprefix").as_deref(), Some("&0&l[&7&lPlayer&0&l] &3"));
        assert_eq!(p.group("nobody", "world", "tabprefix").as_deref(), Some("%luckperms-prefix%"));
        assert_eq!(p.group("Owner", "world1", "tagprefix").as_deref(), Some("&2&lOwner &a"));
        assert_eq!(p.find_group("player").as_deref(), Some("Player"));
    }
}
