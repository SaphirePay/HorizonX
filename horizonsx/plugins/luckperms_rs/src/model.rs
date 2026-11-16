//! LuckPerms' data: nodes, users, groups and tracks, stored as LuckPerms' own JSON storage
//! (`plugins/LuckPerms/json-storage/{users,groups,tracks}/*.json`), and how a user's permissions are worked out
//! (`PermissionCalculator`): their own nodes, then their groups' by inheritance and weight, wildcards last.
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// One node: `permission` (or `group.NAME`, `prefix.W.TEXT`, `suffix.W.TEXT`, `weight.N`, `meta.KEY.VALUE`),
/// its value, the contexts it holds in, and when it ends (unix seconds).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub key: String,
    pub value: bool,
    pub contexts: BTreeMap<String, BTreeSet<String>>,
    pub expiry: Option<i64>,
}

impl Node {
    pub fn new(key: &str, value: bool) -> Self {
        Self { key: key.to_ascii_lowercase_keep_meta(), value, contexts: BTreeMap::new(), expiry: None }
    }
    fn read(v: &Value) -> Option<Self> {
        let key = v["permission"].as_str().or_else(|| v["key"].as_str())?;
        let mut contexts: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        if let Some(map) = v["context"].as_object() {
            for (k, val) in map {
                let set = contexts.entry(k.to_ascii_lowercase()).or_default();
                match val {
                    Value::Array(all) => set.extend(all.iter().filter_map(Value::as_str).map(str::to_ascii_lowercase)),
                    Value::String(one) => {
                        set.insert(one.to_ascii_lowercase());
                    }
                    _ => {}
                }
            }
        }
        Some(Self {
            key: key.to_ascii_lowercase_keep_meta(),
            value: v["value"].as_bool().unwrap_or(true),
            contexts,
            expiry: v["expiry"].as_i64().filter(|&e| e > 0),
        })
    }
    fn write(&self) -> Value {
        let mut out = json!({ "permission": self.key, "value": self.value });
        if !self.contexts.is_empty() {
            let map: Map<String, Value> = self
                .contexts
                .iter()
                .map(|(k, vs)| (k.clone(), Value::Array(vs.iter().map(|v| json!(v)).collect())))
                .collect();
            out["context"] = Value::Object(map);
        }
        if let Some(e) = self.expiry {
            out["expiry"] = json!(e);
        }
        out
    }
    pub fn expired(&self, now: i64) -> bool {
        self.expiry.is_some_and(|e| e <= now)
    }
    /// Holds in `ctx` (each of the node's context keys must have one of its values; `server` "global" holds
    /// everywhere, as LuckPerms' default server name).
    pub fn applies(&self, ctx: &Context) -> bool {
        self.contexts.iter().all(|(k, values)| match k.as_str() {
            "server" => values.contains("global") || values.contains(&ctx.server),
            "world" => values.contains(&ctx.world),
            "gamemode" => values.contains(&ctx.gamemode),
            "dimension-type" => values.contains(&ctx.dimension),
            _ => false,
        })
    }
    /// `group.NAME` -> NAME.
    pub fn group(&self) -> Option<&str> {
        self.key.strip_prefix("group.")
    }
    /// `prefix.W.TEXT` / `suffix.W.TEXT` -> (W, TEXT).
    pub fn chat_meta(&self, kind: &str) -> Option<(i32, &str)> {
        let rest = self.key.strip_prefix(kind)?.strip_prefix('.')?;
        let (weight, text) = rest.split_once('.')?;
        Some((weight.parse().ok()?, text))
    }
    pub fn weight(&self) -> Option<i32> {
        self.key.strip_prefix("weight.")?.parse().ok()
    }
    pub fn is_meta(&self) -> bool {
        self.key.starts_with("prefix.") || self.key.starts_with("suffix.") || self.key.starts_with("weight.") || self.key.starts_with("meta.") || self.key.starts_with("displayname.")
    }
    pub fn describe(&self) -> String {
        let mut out = format!("{} = {}", self.key, self.value);
        for (k, vs) in &self.contexts {
            out.push_str(&format!(" ({k}={})", vs.iter().cloned().collect::<Vec<_>>().join(",")));
        }
        if let Some(e) = self.expiry {
            out.push_str(&format!(" (until {})", format_left(e - now())));
        }
        out
    }
}

/// LuckPerms writes a dot inside a meta key or value as `\.` (`meta.horizonsmp\.claim-blocks.5000`).
pub fn escape_meta(text: &str) -> String {
    text.replace('\\', "\\\\").replace('.', "\\.")
}

/// `KEY.VALUE` (escaped as LuckPerms does) -> (key, value), split at the first dot that isn't escaped.
pub fn split_meta(rest: &str) -> Option<(String, String)> {
    let mut key = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => key.push(chars.next()?),
            '.' => {
                let mut value = String::new();
                let mut rest = chars;
                while let Some(c) = rest.next() {
                    if c == '\\' {
                        if let Some(n) = rest.next() {
                            value.push(n);
                        }
                    } else {
                        value.push(c);
                    }
                }
                return Some((key, value));
            }
            _ => key.push(c),
        }
    }
    None
}

/// Lower case for permission nodes; prefixes, suffixes and meta keep their text as typed.
trait LowerKeepMeta {
    fn to_ascii_lowercase_keep_meta(&self) -> String;
}
impl LowerKeepMeta for str {
    fn to_ascii_lowercase_keep_meta(&self) -> String {
        for kind in ["prefix.", "suffix.", "meta.", "displayname."] {
            if self.len() > kind.len() && self[..kind.len()].eq_ignore_ascii_case(kind) {
                return format!("{kind}{}", &self[kind.len()..]);
            }
        }
        self.to_ascii_lowercase()
    }
}

/// Where a check happens (LuckPerms' contexts).
#[derive(Clone, Debug, Default)]
pub struct Context {
    pub server: String,
    pub world: String,
    pub gamemode: String,
    pub dimension: String,
}

/// A user or a group: its nodes.
#[derive(Clone, Debug, Default)]
pub struct Holder {
    pub name: String,
    pub nodes: Vec<Node>,
}

impl Holder {
    pub fn set(&mut self, node: Node) {
        self.nodes.retain(|n| !(n.key == node.key && n.contexts == node.contexts));
        self.nodes.push(node);
    }
    /// Removes the node with this key and contexts; whether there was one.
    pub fn unset(&mut self, key: &str, contexts: &BTreeMap<String, BTreeSet<String>>) -> bool {
        let before = self.nodes.len();
        self.nodes.retain(|n| !(n.key == key && &n.contexts == contexts));
        before != self.nodes.len()
    }
    pub fn parents(&self, now: i64) -> Vec<String> {
        self.nodes.iter().filter(|n| n.value && !n.expired(now)).filter_map(|n| n.group().map(str::to_owned)).collect()
    }
    pub fn weight(&self) -> i32 {
        self.nodes.iter().filter(|n| n.value).filter_map(Node::weight).max().unwrap_or(0)
    }
    fn drop_expired(&mut self, now: i64) -> bool {
        let before = self.nodes.len();
        self.nodes.retain(|n| !n.expired(now));
        before != self.nodes.len()
    }
}

#[derive(Clone, Debug, Default)]
pub struct User {
    pub uuid: String,
    pub holder: Holder,
    pub primary_group: String,
}

#[derive(Clone, Debug, Default)]
pub struct Track {
    pub name: String,
    pub groups: Vec<String>,
}

/// The JSON storage and what is loaded of it: every group and track, and the users online or asked about.
pub struct Store {
    root: PathBuf,
    pub groups: BTreeMap<String, Holder>,
    pub tracks: BTreeMap<String, Track>,
    pub users: HashMap<String, User>,
    /// name (lower case) -> uuid, from the users' files and everyone seen.
    pub names: HashMap<String, String>,
    pub server: String,
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// `1d2h`, `30m`, `45s`...
pub fn format_left(seconds: i64) -> String {
    let s = seconds.max(0);
    let (d, h, m, sec) = (s / 86400, s % 86400 / 3600, s % 3600 / 60, s % 60);
    let mut out = String::new();
    for (v, u) in [(d, "d"), (h, "h"), (m, "m"), (sec, "s")] {
        if v > 0 {
            out.push_str(&format!("{v}{u}"));
        }
    }
    if out.is_empty() { "0s".to_owned() } else { out }
}

/// `1d`, `12h`, `30m`, `90s`, `1w2d` -> seconds.
pub fn parse_duration(text: &str) -> Option<i64> {
    let mut total = 0i64;
    let mut number = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            number.push(c);
            continue;
        }
        let n: i64 = number.parse().ok()?;
        number.clear();
        total += n * match c {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            'd' => 86400,
            'w' => 604800,
            'y' => 31536000,
            _ => return None,
        };
    }
    if !number.is_empty() {
        total += number.parse::<i64>().ok()?;
    }
    (total > 0).then_some(total)
}

/// A UUID with its dashes (the storage's file names), from either form.
pub fn dashed(uuid: &str) -> String {
    let hex: String = uuid.chars().filter(|c| *c != '-').collect::<String>().to_ascii_lowercase();
    if hex.len() != 32 {
        return uuid.to_ascii_lowercase();
    }
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}

fn read_nodes(v: &Value) -> Vec<Node> {
    v["permissions"].as_array().or_else(|| v["nodes"].as_array()).into_iter().flatten().filter_map(Node::read).collect()
}

impl Store {
    pub fn open(folder: &Path, server: &str) -> Self {
        let root = folder.join("json-storage");
        for part in ["users", "groups", "tracks"] {
            let _ = std::fs::create_dir_all(root.join(part));
        }
        let mut store = Self {
            root,
            groups: BTreeMap::new(),
            tracks: BTreeMap::new(),
            users: HashMap::new(),
            names: HashMap::new(),
            server: server.to_ascii_lowercase(),
        };
        store.reload();
        store
    }
    /// Every group and track again, from the files (`/lp sync`); the users loaded stay, read again.
    pub fn reload(&mut self) {
        self.groups.clear();
        self.tracks.clear();
        for (part, is_group) in [("groups", true), ("tracks", false)] {
            let Ok(dir) = std::fs::read_dir(self.root.join(part)) else { continue };
            for entry in dir.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else { continue };
                let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_ascii_lowercase();
                let name = v["name"].as_str().map_or(stem, str::to_ascii_lowercase);
                if is_group {
                    self.groups.insert(name.clone(), Holder { name, nodes: read_nodes(&v) });
                } else {
                    let groups = v["groups"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_ascii_lowercase).collect();
                    self.tracks.insert(name.clone(), Track { name, groups });
                }
            }
        }
        if !self.groups.contains_key("default") {
            self.groups.insert("default".into(), Holder { name: "default".into(), nodes: Vec::new() });
            self.save_group("default");
        }
        // Who is who, from the users' files.
        if let Ok(dir) = std::fs::read_dir(self.root.join("users")) {
            for entry in dir.flatten() {
                let path = entry.path();
                let Some(uuid) = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else { continue };
                if let Ok(text) = std::fs::read_to_string(&path)
                    && let Ok(v) = serde_json::from_str::<Value>(&text)
                    && let Some(name) = v["name"].as_str()
                {
                    self.names.insert(name.to_ascii_lowercase(), dashed(&uuid));
                }
            }
        }
        let loaded: Vec<(String, String)> = self.users.values().map(|u| (u.uuid.clone(), u.holder.name.clone())).collect();
        self.users.clear();
        for (uuid, name) in loaded {
            self.load_user(&uuid, &name);
        }
    }
    /// The user's file read (or a new user in `default`), kept loaded.
    pub fn load_user(&mut self, uuid: &str, name: &str) -> &mut User {
        let uuid = dashed(uuid);
        if !name.is_empty() {
            self.names.insert(name.to_ascii_lowercase(), uuid.clone());
        }
        if !self.users.contains_key(&uuid) {
            let path = self.root.join("users").join(format!("{uuid}.json"));
            let mut user = match std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
                Some(v) => User {
                    uuid: uuid.clone(),
                    holder: Holder { name: v["name"].as_str().unwrap_or(name).to_owned(), nodes: read_nodes(&v) },
                    primary_group: v["primaryGroup"].as_str().unwrap_or("default").to_ascii_lowercase(),
                },
                None => User { uuid: uuid.clone(), holder: Holder { name: name.to_owned(), nodes: Vec::new() }, primary_group: "default".into() },
            };
            // Everyone is in `default` unless they're in another group (LuckPerms' default assignment).
            if user.holder.parents(now()).is_empty() {
                user.holder.set(Node::new("group.default", true));
            }
            if !name.is_empty() && user.holder.name != name {
                user.holder.name = name.to_owned();
            }
            self.users.insert(uuid.clone(), user);
        }
        self.users.get_mut(&uuid).expect("loaded above")
    }
    pub fn unload_user(&mut self, uuid: &str) {
        self.users.remove(&dashed(uuid));
    }
    pub fn uuid_of(&self, name: &str) -> Option<String> {
        self.names.get(&name.to_ascii_lowercase()).cloned()
    }
    pub fn save_user(&self, uuid: &str) {
        let Some(user) = self.users.get(&dashed(uuid)) else { return };
        // A user holding only `group.default` is LuckPerms' "nothing to save": its file is dropped.
        let only_default = user.holder.nodes.iter().all(|n| n.key == "group.default" && n.value && n.contexts.is_empty() && n.expiry.is_none());
        let path = self.root.join("users").join(format!("{}.json", user.uuid));
        if only_default && user.primary_group == "default" {
            let _ = std::fs::remove_file(path);
            return;
        }
        let v = json!({
            "uuid": user.uuid,
            "name": user.holder.name,
            "primaryGroup": user.primary_group,
            "permissions": user.holder.nodes.iter().map(Node::write).collect::<Vec<_>>(),
        });
        write_json(&path, &v);
    }
    pub fn save_group(&self, name: &str) {
        let path = self.root.join("groups").join(format!("{name}.json"));
        match self.groups.get(name) {
            Some(group) => {
                let mut v = json!({ "name": group.name });
                if !group.nodes.is_empty() {
                    v["permissions"] = Value::Array(group.nodes.iter().map(Node::write).collect());
                }
                write_json(&path, &v);
            }
            None => {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    pub fn save_track(&self, name: &str) {
        let path = self.root.join("tracks").join(format!("{name}.json"));
        match self.tracks.get(name) {
            Some(track) => write_json(&path, &json!({ "name": track.name, "groups": track.groups })),
            None => {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    /// Temporary nodes past their time, dropped (and saved): what changed.
    pub fn drop_expired(&mut self) -> (Vec<String>, Vec<String>) {
        let now = now();
        let mut users = Vec::new();
        let mut groups = Vec::new();
        for (uuid, user) in &mut self.users {
            if user.holder.drop_expired(now) {
                users.push(uuid.clone());
            }
        }
        for (name, group) in &mut self.groups {
            if group.drop_expired(now) {
                groups.push(name.clone());
            }
        }
        for u in &users {
            self.save_user(u);
        }
        for g in &groups {
            self.save_group(g);
        }
        (users, groups)
    }
    /// The groups a holder inherits, nearest first, then by weight (LuckPerms' inheritance order), each once.
    pub fn inherited(&self, holder: &Holder, ctx: &Context, now: i64) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut frontier: Vec<String> = parents_in(holder, ctx, now);
        while !frontier.is_empty() {
            frontier.sort_by_key(|g| std::cmp::Reverse(self.groups.get(g).map_or(0, Holder::weight)));
            let mut next = Vec::new();
            for g in frontier {
                if out.contains(&g) {
                    continue;
                }
                let Some(group) = self.groups.get(&g) else { continue };
                out.push(g.clone());
                next.extend(parents_in(group, ctx, now));
            }
            frontier = next;
        }
        out
    }
    /// Every permission the user holds in `ctx`, worked out as LuckPerms does: the user's own nodes first, then
    /// each inherited group's in inheritance order; a node set earlier wins. Groups appear as `group.NAME`.
    pub fn resolve(&self, user: &User, ctx: &Context) -> BTreeMap<String, bool> {
        let now = now();
        let mut out: BTreeMap<String, bool> = BTreeMap::new();
        let take = |holder: &Holder, out: &mut BTreeMap<String, bool>| {
            for n in holder.nodes.iter().filter(|n| !n.expired(now) && n.applies(ctx) && !n.is_meta()) {
                out.entry(n.key.clone()).or_insert(n.value);
            }
        };
        take(&user.holder, &mut out);
        for g in self.inherited(&user.holder, ctx, now) {
            if let Some(group) = self.groups.get(&g) {
                take(group, &mut out);
                out.entry(format!("group.{g}")).or_insert(true);
            }
        }
        out
    }
    /// Every prefix and suffix (highest weight first) and the `meta.KEY.VALUE` values (the nearest holder's
    /// wins: the user's, then the groups' in inheritance order) the user has in `ctx`.
    pub fn all_meta(&self, user: &User, ctx: &Context) -> (Vec<String>, Vec<String>, BTreeMap<String, String>) {
        let now = now();
        let (mut prefixes, mut suffixes): (Vec<(i32, String)>, Vec<(i32, String)>) = (Vec::new(), Vec::new());
        let mut meta: BTreeMap<String, String> = BTreeMap::new();
        let mut look = |holder: &Holder| {
            for n in holder.nodes.iter().filter(|n| n.value && !n.expired(now) && n.applies(ctx)) {
                if let Some((w, t)) = n.chat_meta("prefix") {
                    prefixes.push((w, t.to_owned()));
                } else if let Some((w, t)) = n.chat_meta("suffix") {
                    suffixes.push((w, t.to_owned()));
                } else if let Some((k, v)) = n.key.strip_prefix("meta.").and_then(split_meta) {
                    meta.entry(k.to_ascii_lowercase()).or_insert(v);
                }
            }
        };
        look(&user.holder);
        for g in self.inherited(&user.holder, ctx, now) {
            if let Some(group) = self.groups.get(&g) {
                look(group);
            }
        }
        let order = |mut v: Vec<(i32, String)>| {
            v.sort_by(|a, b| b.0.cmp(&a.0));
            v.into_iter().map(|(_, t)| t).collect::<Vec<_>>()
        };
        (order(prefixes), order(suffixes), meta)
    }
    /// The user's prefix or suffix in `ctx`: the highest weight among theirs and their groups'.
    pub fn chat_meta(&self, user: &User, ctx: &Context, kind: &str) -> Option<String> {
        let now = now();
        let mut best: Option<(i32, String)> = None;
        let mut look = |holder: &Holder| {
            for n in holder.nodes.iter().filter(|n| n.value && !n.expired(now) && n.applies(ctx)) {
                if let Some((w, text)) = n.chat_meta(kind)
                    && best.as_ref().is_none_or(|(bw, _)| w > *bw)
                {
                    best = Some((w, text.to_owned()));
                }
            }
        };
        look(&user.holder);
        for g in self.inherited(&user.holder, ctx, now) {
            if let Some(group) = self.groups.get(&g) {
                look(group);
            }
        }
        best.map(|(_, t)| t)
    }
}

fn parents_in(holder: &Holder, ctx: &Context, now: i64) -> Vec<String> {
    holder.nodes.iter().filter(|n| n.value && !n.expired(now) && n.applies(ctx)).filter_map(|n| n.group().map(str::to_owned)).collect()
}

/// Written beside, then moved over: a crash mid-write never leaves half a file.
fn write_json(path: &Path, v: &Value) {
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_string_pretty(v).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

/// What the engine is told for each node the user resolves to: wildcards (`a.b.*`, `*`) spread over every
/// permission the server's plugins declared under them (`known`), a node named outright winning over a wildcard
/// and a longer wildcard over a shorter one (LuckPerms' `WildcardProcessor`).
pub fn expand(resolved: &BTreeMap<String, bool>, known: &[String]) -> BTreeMap<String, bool> {
    let mut out: BTreeMap<String, bool> = BTreeMap::new();
    let mut wildcards: Vec<(&str, bool)> = Vec::new();
    for (node, &value) in resolved {
        if node == "*" || node.ends_with(".*") {
            wildcards.push((node.as_str(), value));
        }
        out.insert(node.clone(), value);
    }
    // Longest first: `essentials.home.*` before `essentials.*` before `*`.
    wildcards.sort_by_key(|(w, _)| std::cmp::Reverse(w.len()));
    for perm in known {
        if out.contains_key(perm) {
            continue;
        }
        for (w, value) in &wildcards {
            let prefix = w.trim_end_matches('*');
            if prefix.is_empty() || perm.starts_with(prefix) {
                out.insert(perm.clone(), *value);
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Context {
        Context { server: "global".into(), world: "world".into(), gamemode: "survival".into(), dimension: "overworld".into() }
    }

    fn store() -> Store {
        let dir = std::env::temp_dir().join(format!("lprs-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Store::open(&dir, "global")
    }

    #[test]
    fn a_users_own_node_wins_over_a_groups_and_heavier_groups_come_first() {
        let mut s = store();
        let mut admin = Holder { name: "admin".into(), nodes: vec![Node::new("weight.100", true), Node::new("fly", true), Node::new("build", false)] };
        admin.set(Node::new("group.default", true));
        s.groups.insert("admin".into(), admin);
        let mut vip = Holder { name: "vip".into(), nodes: vec![Node::new("weight.10", true), Node::new("build", true)] };
        vip.set(Node::new("group.default", true));
        s.groups.insert("vip".into(), vip);
        s.groups.get_mut("default").unwrap().set(Node::new("spawn", true));
        let user = s.load_user("00000000-0000-0000-0000-000000000001", "Steve").clone();
        let mut user = user;
        user.holder.set(Node::new("group.vip", true));
        user.holder.set(Node::new("group.admin", true));
        user.holder.set(Node::new("fly", false));
        let r = s.resolve(&user, &ctx());
        assert_eq!(r.get("fly"), Some(&false), "the user's own node wins");
        assert_eq!(r.get("build"), Some(&false), "admin (weight 100) before vip (10)");
        assert_eq!(r.get("spawn"), Some(&true), "default reached through both");
        assert_eq!(r.get("group.admin"), Some(&true));
    }

    #[test]
    fn contexts_expiry_and_wildcards() {
        let mut s = store();
        let mut user = s.load_user("00000000-0000-0000-0000-000000000002", "Alex").clone();
        let mut nether = Node::new("nether.only", true);
        nether.contexts.insert("world".into(), ["world_nether".to_owned()].into());
        user.holder.set(nether);
        let mut gone = Node::new("old.perm", true);
        gone.expiry = Some(now() - 5);
        user.holder.set(gone);
        user.holder.set(Node::new("essentials.*", true));
        user.holder.set(Node::new("essentials.god", false));
        let r = s.resolve(&user, &ctx());
        assert!(!r.contains_key("nether.only") && !r.contains_key("old.perm"));
        let known = vec!["essentials.fly".to_owned(), "essentials.god".to_owned(), "other.x".to_owned()];
        let e = expand(&r, &known);
        assert_eq!(e.get("essentials.fly"), Some(&true));
        assert_eq!(e.get("essentials.god"), Some(&false), "a node named outright beats the wildcard");
        assert!(!e.contains_key("other.x"));
    }

    #[test]
    fn storage_round_trips_luckperms_json() {
        let s = store();
        let mut s = s;
        let u = s.load_user("00000000000000000000000000000003", "Bob");
        let mut node = Node::new("prefix.100.&c[Admin] ", true);
        node.contexts.insert("server".into(), ["global".to_owned()].into());
        u.holder.set(node);
        u.holder.set(Node::new("group.admin", true));
        s.save_user("00000000-0000-0000-0000-000000000003");
        s.unload_user("00000000-0000-0000-0000-000000000003");
        let back = s.load_user("00000000-0000-0000-0000-000000000003", "Bob").clone();
        assert!(back.holder.nodes.iter().any(|n| n.key == "prefix.100.&c[Admin] "), "{:?}", back.holder.nodes);
        assert!(back.holder.nodes.iter().any(|n| n.key == "group.admin"));
        assert_eq!(parse_duration("1d2h"), Some(93600));
        assert_eq!(dashed("0000000000000000000000000000000A"), "00000000-0000-0000-0000-00000000000a");
    }
}
