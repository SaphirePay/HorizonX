//! `/lp` (`/luckperms`): LuckPerms' commands for users, groups and tracks, with the same words and arguments.
use crate::model::{self, Holder, Node, Track};
use horizon_bukkit::CommandSender;
use std::collections::{BTreeMap, BTreeSet};

const PREFIX: &str = "\u{a7}7[\u{a7}bL\u{a7}3P\u{a7}7] \u{a7}r";

fn say(sender: &CommandSender, text: &str) {
    for line in text.lines() {
        sender.send_message(&format!("{PREFIX}{}", line.replace('&', "\u{a7}")));
    }
}

fn allowed(sender: &CommandSender, node: &str) -> bool {
    sender.has_permission("luckperms.*") || sender.has_permission(node)
}

/// What a command changes: a user (by dashed uuid) or a group.
enum Target {
    User(String),
    Group(String),
}

impl Target {
    fn label(&self) -> String {
        match self {
            Target::User(uuid) => crate::with(|s| s.store.users.get(uuid).map(|u| u.holder.name.clone())).flatten().unwrap_or_else(|| uuid.clone()),
            Target::Group(g) => g.clone(),
        }
    }
    /// `f` on the target's nodes; saved and applied if it says it changed something.
    fn edit<R>(&self, f: impl FnOnce(&mut Holder) -> (R, bool)) -> Option<R> {
        let (out, changed) = crate::with(|s| {
            let holder = match self {
                Target::User(uuid) => &mut s.store.users.get_mut(uuid)?.holder,
                Target::Group(g) => s.store.groups.get_mut(g)?,
            };
            let (out, changed) = f(holder);
            if changed {
                match self {
                    Target::User(uuid) => s.store.save_user(uuid),
                    Target::Group(g) => s.store.save_group(g),
                }
            }
            Some((out, changed))
        })??;
        if changed {
            match self {
                Target::User(uuid) => {
                    if let Some(p) = crate::online(uuid) {
                        crate::apply(&p);
                    }
                }
                Target::Group(_) => crate::apply_all(),
            }
        }
        Some(out)
    }
    fn nodes(&self) -> Vec<Node> {
        crate::with(|s| match self {
            Target::User(uuid) => s.store.users.get(uuid).map(|u| u.holder.nodes.clone()),
            Target::Group(g) => s.store.groups.get(g).map(|h| h.nodes.clone()),
        })
        .flatten()
        .unwrap_or_default()
    }
}

/// `key=value` words: the node's contexts.
fn contexts(words: &[&str]) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for w in words {
        if let Some((k, v)) = w.split_once('=') {
            out.entry(k.to_ascii_lowercase()).or_default().insert(v.to_ascii_lowercase());
        } else {
            // A lone word is a server name, as LuckPerms reads `<server> <world>` in older syntax.
            out.entry("server".into()).or_default().insert(w.to_ascii_lowercase());
        }
    }
    out
}

fn bool_word(w: Option<&&str>) -> (bool, usize) {
    match w.map(|w| w.to_ascii_lowercase()) {
        Some(ref v) if v == "true" => (true, 1),
        Some(ref v) if v == "false" => (false, 1),
        _ => (true, 0),
    }
}

pub fn run(sender: &CommandSender, args: &[&str]) {
    let Some(first) = args.first().map(|a| a.to_ascii_lowercase()) else {
        say(sender, "&2Running &bLuckPerms v5.5.71 &7(Horizon, Rust)&2. Use &a/lp help&2.");
        return;
    };
    let rest = &args[1..];
    match first.as_str() {
        "help" => help(sender),
        "info" => {
            if !allowed(sender, "luckperms.info") {
                return say(sender, "&cYou do not have permission to use this command!");
            }
            let (groups, tracks, users) = crate::with(|s| (s.store.groups.len(), s.store.tracks.len(), s.store.users.len())).unwrap_or_default();
            say(sender, &format!("&2Running &bLuckPerms v5.5.71 &7(Horizon, Rust)\n&f- &3Storage: &fJSON\n&f- &3Groups: &f{groups}  &3Tracks: &f{tracks}  &3Users loaded: &f{users}"));
        }
        "sync" | "reloadconfig" | "reload" => {
            if !allowed(sender, "luckperms.sync") {
                return say(sender, "&cYou do not have permission to use this command!");
            }
            crate::with(|s| s.store.reload());
            crate::apply_all();
            say(sender, "&bReloaded the storage and every online player's permissions.");
        }
        "creategroup" => {
            if !allowed(sender, "luckperms.creategroup") {
                return say(sender, "&cYou do not have permission to use this command!");
            }
            let Some(name) = rest.first().map(|n| n.to_ascii_lowercase()) else { return say(sender, "&cUsage: /lp creategroup <name> [weight]") };
            let weight = rest.get(1).and_then(|w| w.parse::<i32>().ok());
            let made = crate::with(|s| {
                if s.store.groups.contains_key(&name) {
                    return false;
                }
                let mut h = Holder { name: name.clone(), nodes: Vec::new() };
                if let Some(w) = weight {
                    h.set(Node::new(&format!("weight.{w}"), true));
                }
                s.store.groups.insert(name.clone(), h);
                s.store.save_group(&name);
                true
            })
            .unwrap_or(false);
            say(sender, &if made { format!("&b{name}&a was successfully created.") } else { format!("&c{name} already exists!") });
        }
        "deletegroup" => {
            if !allowed(sender, "luckperms.deletegroup") {
                return say(sender, "&cYou do not have permission to use this command!");
            }
            let Some(name) = rest.first().map(|n| n.to_ascii_lowercase()) else { return say(sender, "&cUsage: /lp deletegroup <name>") };
            if name == "default" {
                return say(sender, "&cThe default group cannot be deleted.");
            }
            let gone = crate::with(|s| {
                let gone = s.store.groups.remove(&name).is_some();
                s.store.save_group(&name);
                gone
            })
            .unwrap_or(false);
            if gone {
                crate::apply_all();
            }
            say(sender, &if gone { format!("&b{name}&a was successfully deleted.") } else { format!("&c{name} does not exist!") });
        }
        "listgroups" => {
            let lines = crate::with(|s| {
                let mut gs: Vec<(i32, String)> = s.store.groups.values().map(|g| (g.weight(), g.name.clone())).collect();
                gs.sort_by_key(|(w, n)| (std::cmp::Reverse(*w), n.clone()));
                gs.into_iter().map(|(w, n)| format!("&f- &b{n} &7(weight {w})")).collect::<Vec<_>>().join("\n")
            })
            .unwrap_or_default();
            say(sender, &format!("&aGroups:\n{lines}"));
        }
        "createtrack" => {
            if !allowed(sender, "luckperms.createtrack") {
                return say(sender, "&cYou do not have permission to use this command!");
            }
            let Some(name) = rest.first().map(|n| n.to_ascii_lowercase()) else { return say(sender, "&cUsage: /lp createtrack <name>") };
            let made = crate::with(|s| {
                if s.store.tracks.contains_key(&name) {
                    return false;
                }
                s.store.tracks.insert(name.clone(), Track { name: name.clone(), groups: Vec::new() });
                s.store.save_track(&name);
                true
            })
            .unwrap_or(false);
            say(sender, &if made { format!("&b{name}&a was successfully created.") } else { format!("&c{name} already exists!") });
        }
        "deletetrack" => {
            if !allowed(sender, "luckperms.deletetrack") {
                return say(sender, "&cYou do not have permission to use this command!");
            }
            let Some(name) = rest.first().map(|n| n.to_ascii_lowercase()) else { return say(sender, "&cUsage: /lp deletetrack <name>") };
            let gone = crate::with(|s| {
                let gone = s.store.tracks.remove(&name).is_some();
                s.store.save_track(&name);
                gone
            })
            .unwrap_or(false);
            say(sender, &if gone { format!("&b{name}&a was successfully deleted.") } else { format!("&c{name} does not exist!") });
        }
        "listtracks" => {
            let lines = crate::with(|s| s.store.tracks.values().map(|t| format!("&f- &b{} &7({})", t.name, t.groups.join(" > "))).collect::<Vec<_>>().join("\n")).unwrap_or_default();
            say(sender, &format!("&aTracks:\n{lines}"));
        }
        "user" => user(sender, rest),
        "group" => group(sender, rest),
        "track" => track(sender, rest),
        _ => say(sender, "&cUnknown command. Use &a/lp help&c."),
    }
}

fn help(sender: &CommandSender) {
    say(sender, "&3LuckPerms commands:\n\
&b/lp info&7, &b/lp sync&7, &b/lp listgroups&7, &b/lp listtracks\n\
&b/lp creategroup <g> [weight]&7, &b/lp deletegroup <g>&7, &b/lp createtrack <t>&7, &b/lp deletetrack <t>\n\
&b/lp user <u> info | permission set/unset/settemp/check <node> [true|false] [duration] [ctx=..]\n\
&b/lp user <u> parent add/remove/set/addtemp <g> | promote/demote <track> | meta set/unset <key> [value] | meta setprefix/setsuffix <w> <text>\n\
&b/lp group <g> info | permission ... | parent ... | meta ... | setweight <n> | listmembers\n\
&b/lp track <t> info | append <g> | insert <g> <pos> | remove <g> | clear");
}

/// `/lp user <name|uuid> ...`
fn user(sender: &CommandSender, args: &[&str]) {
    let Some(who) = args.first() else { return say(sender, "&cUsage: /lp user <user> <action>") };
    let uuid = if who.len() >= 32 && who.chars().filter(|c| *c != '-').all(|c| c.is_ascii_hexdigit()) {
        Some(model::dashed(who))
    } else {
        horizon_bukkit::get_player(who).map(|p| model::dashed(&p.get_unique_id())).or_else(|| crate::with(|s| s.store.uuid_of(who)).flatten())
    };
    let Some(uuid) = uuid else { return say(sender, &format!("&cA user for &4{who}&c could not be found.")) };
    let name = horizon_bukkit::get_player(who).map(|p| p.get_name().to_owned()).unwrap_or_else(|| (*who).to_owned());
    crate::with(|s| {
        s.store.load_user(&uuid, &name);
    });
    let target = Target::User(uuid.clone());
    let rest = &args[1..];
    match rest.first().map(|a| a.to_ascii_lowercase()).as_deref() {
        None | Some("info") => {
            let info = crate::with(|s| {
                let user = s.store.users.get(&uuid)?.clone();
                let ctx = crate::online(&uuid).map(|p| crate::context_of(&p, &s.store.server.clone())).unwrap_or_default();
                let groups = s.store.inherited(&user.holder, &ctx, model::now());
                let prefix = s.store.chat_meta(&user, &ctx, "prefix").unwrap_or_default();
                let suffix = s.store.chat_meta(&user, &ctx, "suffix").unwrap_or_default();
                Some(format!(
                    "&b&l> &bUser Info: &f{}\n&f- &3UUID: &f{}\n&f- &3Status: {}\n&f- &3Primary group: &f{}\n&f- &3Parent groups: &f{}\n&f- &3Prefix: &f\"{}\"\n&f- &3Suffix: &f\"{}\"",
                    user.holder.name,
                    user.uuid,
                    if crate::online(&uuid).is_some() { "&aOnline" } else { "&cOffline" },
                    user.primary_group,
                    groups.join(", "),
                    prefix,
                    suffix
                ))
            })
            .flatten()
            .unwrap_or_default();
            say(sender, &info);
        }
        Some("permission") => permission(sender, &target, "user", &rest[1..]),
        Some("parent") => parent(sender, &target, "user", &rest[1..]),
        Some("meta") => meta(sender, &target, "user", &rest[1..]),
        Some("promote") | Some("demote") => {
            let up = rest[0].eq_ignore_ascii_case("promote");
            if !allowed(sender, &format!("luckperms.user.{}", if up { "promote" } else { "demote" })) {
                return say(sender, "&cYou do not have permission to use this command!");
            }
            let Some(track) = rest.get(1).map(|t| t.to_ascii_lowercase()) else { return say(sender, "&cUsage: /lp user <user> promote|demote <track>") };
            let Some(groups) = crate::with(|s| s.store.tracks.get(&track).map(|t| t.groups.clone())).flatten() else {
                return say(sender, &format!("&c{track} does not exist!"));
            };
            let result = target.edit(|h| {
                let now = model::now();
                let held: Vec<String> = h.parents(now);
                let at = groups.iter().position(|g| held.contains(g));
                let (from, to) = match (at, up) {
                    (None, true) => (None, groups.first().cloned()),
                    (None, false) => return (Err("is not on this track".to_owned()), false),
                    (Some(i), true) => (Some(groups[i].clone()), groups.get(i + 1).cloned()),
                    (Some(i), false) => (Some(groups[i].clone()), i.checked_sub(1).map(|j| groups[j].clone())),
                };
                let Some(to) = to else {
                    return (Err(if up { "is already at the top of this track" } else { "is already at the bottom of this track" }.to_owned()), false);
                };
                if let Some(from) = &from {
                    h.unset(&format!("group.{from}"), &BTreeMap::new());
                }
                h.set(Node::new(&format!("group.{to}"), true));
                (Ok((from, to)), true)
            });
            match result {
                Some(Ok((from, to))) => say(sender, &format!("&a{} &bhas been {} from &a{}&b to &a{}&b on &a{track}&b.", target.label(), if up { "promoted" } else { "demoted" }, from.unwrap_or_else(|| "nothing".into()), to)),
                Some(Err(why)) => say(sender, &format!("&c{} {why}.", target.label())),
                None => {}
            }
        }
        Some("clear") => {
            if !allowed(sender, "luckperms.user.clear") {
                return say(sender, "&cYou do not have permission to use this command!");
            }
            target.edit(|h| {
                h.nodes.clear();
                h.set(Node::new("group.default", true));
                ((), true)
            });
            say(sender, &format!("&b{}&a's nodes were cleared.", target.label()));
        }
        _ => say(sender, "&cUnknown action. Use &a/lp help&c."),
    }
}

/// `/lp group <name> ...`
fn group(sender: &CommandSender, args: &[&str]) {
    let Some(name) = args.first().map(|g| g.to_ascii_lowercase()) else { return say(sender, "&cUsage: /lp group <group> <action>") };
    if crate::with(|s| s.store.groups.contains_key(&name)) != Some(true) {
        return say(sender, &format!("&c{name} does not exist!"));
    }
    let target = Target::Group(name.clone());
    let rest = &args[1..];
    match rest.first().map(|a| a.to_ascii_lowercase()).as_deref() {
        None | Some("info") => {
            let nodes = target.nodes();
            let weight = nodes.iter().filter_map(Node::weight).max().unwrap_or(0);
            let parents: Vec<String> = nodes.iter().filter(|n| n.value).filter_map(|n| n.group().map(str::to_owned)).collect();
            say(sender, &format!("&b&l> &bGroup Info: &f{name}\n&f- &3Weight: &f{weight}\n&f- &3Parent groups: &f{}\n&f- &3Nodes: &f{}", parents.join(", "), nodes.len()));
        }
        Some("permission") => permission(sender, &target, "group", &rest[1..]),
        Some("parent") => parent(sender, &target, "group", &rest[1..]),
        Some("meta") => meta(sender, &target, "group", &rest[1..]),
        Some("setweight") => {
            if !allowed(sender, "luckperms.group.setweight") {
                return say(sender, "&cYou do not have permission to use this command!");
            }
            let Some(w) = rest.get(1).and_then(|w| w.parse::<i32>().ok()) else { return say(sender, "&cUsage: /lp group <group> setweight <weight>") };
            target.edit(|h| {
                h.nodes.retain(|n| n.weight().is_none());
                h.set(Node::new(&format!("weight.{w}"), true));
                ((), true)
            });
            say(sender, &format!("&aSet weight to &b{w}&a for group &b{name}&a."));
        }
        Some("listmembers") => {
            let members = crate::with(|s| {
                s.store
                    .users
                    .values()
                    .filter(|u| u.holder.nodes.iter().any(|n| n.group() == Some(name.as_str()) && n.value))
                    .map(|u| u.holder.name.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
            say(sender, &format!("&aMembers of &b{name}&a (loaded): &f{}", members.join(", ")));
        }
        _ => say(sender, "&cUnknown action. Use &a/lp help&c."),
    }
}

/// `/lp track <name> ...`
fn track(sender: &CommandSender, args: &[&str]) {
    let Some(name) = args.first().map(|t| t.to_ascii_lowercase()) else { return say(sender, "&cUsage: /lp track <track> <action>") };
    let rest = &args[1..];
    let action = rest.first().map(|a| a.to_ascii_lowercase());
    if action.as_deref().is_some_and(|a| a != "info") && !allowed(sender, &format!("luckperms.track.{}", action.as_deref().unwrap_or(""))) {
        return say(sender, "&cYou do not have permission to use this command!");
    }
    let out = crate::with(|s| {
        let groups_known: Vec<String> = s.store.groups.keys().cloned().collect();
        let Some(t) = s.store.tracks.get_mut(&name) else { return format!("&c{name} does not exist!") };
        let g = rest.get(1).map(|g| g.to_ascii_lowercase());
        let msg = match (action.as_deref(), g) {
            (None | Some("info"), _) => return format!("&b&l> &bTrack: &f{name}\n&f- &3Path: &f{}", t.groups.join(" > ")),
            (Some("append"), Some(g)) if groups_known.contains(&g) && !t.groups.contains(&g) => {
                t.groups.push(g.clone());
                format!("&aAppended &b{g}&a to &b{name}&a.")
            }
            (Some("insert"), Some(g)) if groups_known.contains(&g) && !t.groups.contains(&g) => {
                let at = rest.get(2).and_then(|p| p.parse::<usize>().ok()).unwrap_or(1).saturating_sub(1).min(t.groups.len());
                t.groups.insert(at, g.clone());
                format!("&aInserted &b{g}&a into &b{name}&a at position {}.", at + 1)
            }
            (Some("remove"), Some(g)) if t.groups.contains(&g) => {
                t.groups.retain(|x| *x != g);
                format!("&aRemoved &b{g}&a from &b{name}&a.")
            }
            (Some("clear"), _) => {
                t.groups.clear();
                format!("&aCleared &b{name}&a.")
            }
            _ => return "&cUsage: /lp track <track> info|append <group>|insert <group> <pos>|remove <group>|clear (the group must exist, once on the track)".to_owned(),
        };
        s.store.save_track(&name);
        msg
    })
    .unwrap_or_default();
    say(sender, &out);
}

/// `... permission info|set|unset|settemp|unsettemp|check`
fn permission(sender: &CommandSender, target: &Target, kind: &str, args: &[&str]) {
    let action = args.first().map(|a| a.to_ascii_lowercase()).unwrap_or_else(|| "info".into());
    if !allowed(sender, &format!("luckperms.{kind}.permission.{action}")) {
        return say(sender, "&cYou do not have permission to use this command!");
    }
    let label = target.label();
    match action.as_str() {
        "info" => {
            let list: Vec<String> = target.nodes().iter().filter(|n| n.group().is_none() && !n.is_meta()).map(|n| format!("&f- &3{}", n.describe())).collect();
            say(sender, &format!("&b{label}&a's permissions:\n{}", if list.is_empty() { "&7(none)".to_owned() } else { list.join("\n") }));
        }
        "set" | "settemp" => {
            let Some(node) = args.get(1) else { return say(sender, &format!("&cUsage: /lp {kind} <name> permission {action} <node> [true|false]{} [context...]", if action == "settemp" { " <duration>" } else { "" })) };
            let (value, used) = bool_word(args.get(2));
            let mut at = 2 + used;
            let mut expiry = None;
            if action == "settemp" {
                let Some(secs) = args.get(at).and_then(|d| model::parse_duration(d)) else { return say(sender, "&cGive a duration: 1d, 12h, 30m...") };
                expiry = Some(model::now() + secs);
                at += 1;
            }
            let mut n = Node::new(node, value);
            n.contexts = contexts(&args[at.min(args.len())..]);
            n.expiry = expiry;
            let text = n.describe();
            target.edit(|h| {
                h.set(n);
                ((), true)
            });
            say(sender, &format!("&aSet &b{text}&a for &b{label}&a."));
        }
        "unset" | "unsettemp" => {
            let Some(node) = args.get(1) else { return say(sender, &format!("&cUsage: /lp {kind} <name> permission unset <node> [context...]")) };
            let key = Node::new(node, true).key;
            let ctx = contexts(&args[2.min(args.len())..]);
            let temp = action == "unsettemp";
            let removed = target
                .edit(|h| {
                    let before = h.nodes.len();
                    h.nodes.retain(|n| !(n.key == key && n.contexts == ctx && n.expiry.is_some() == temp));
                    let changed = h.nodes.len() != before;
                    (changed, changed)
                })
                .unwrap_or(false);
            say(sender, &if removed { format!("&aUnset &b{key}&a for &b{label}&a.") } else { format!("&b{label}&c does not have &b{key}&c set.") });
        }
        "check" => {
            let Some(node) = args.get(1) else { return say(sender, &format!("&cUsage: /lp {kind} <name> permission check <node>")) };
            let key = node.to_ascii_lowercase();
            let answer = crate::with(|s| {
                let ctx = match target {
                    Target::User(u) => crate::online(u).map(|p| crate::context_of(&p, &s.store.server.clone())).unwrap_or_default(),
                    Target::Group(_) => model::Context { server: s.store.server.clone(), ..Default::default() },
                };
                let resolved = match target {
                    Target::User(u) => s.store.users.get(u).map(|user| s.store.resolve(user, &ctx)),
                    Target::Group(g) => s.store.groups.get(g).map(|h| {
                        let fake = model::User { uuid: String::new(), holder: h.clone(), primary_group: g.clone() };
                        s.store.resolve(&fake, &ctx)
                    }),
                }
                .unwrap_or_default();
                model::expand(&resolved, &[key.clone()]).get(&key).copied()
            })
            .flatten();
            say(sender, &format!("&b{label}&a has &b{key}&a set to {}", match answer {
                Some(true) => "&2true",
                Some(false) => "&cfalse",
                None => "&7undefined",
            }));
        }
        _ => say(sender, "&cUsage: permission info|set|unset|settemp|unsettemp|check"),
    }
}

/// `... parent info|add|remove|set|addtemp`
fn parent(sender: &CommandSender, target: &Target, kind: &str, args: &[&str]) {
    let action = args.first().map(|a| a.to_ascii_lowercase()).unwrap_or_else(|| "info".into());
    if !allowed(sender, &format!("luckperms.{kind}.parent.{action}")) {
        return say(sender, "&cYou do not have permission to use this command!");
    }
    let label = target.label();
    if action == "info" {
        let parents: Vec<String> = target.nodes().iter().filter_map(|n| n.group().map(|g| format!("&f- &b{g}{}", n.expiry.map_or(String::new(), |e| format!(" &7({} left)", model::format_left(e - model::now())))))).collect();
        return say(sender, &format!("&b{label}&a's parents:\n{}", parents.join("\n")));
    }
    let Some(g) = args.get(1).map(|g| g.to_ascii_lowercase()) else { return say(sender, &format!("&cUsage: /lp {kind} <name> parent {action} <group>")) };
    if crate::with(|s| s.store.groups.contains_key(&g)) != Some(true) {
        return say(sender, &format!("&c{g} does not exist!"));
    }
    if matches!(target, Target::Group(name) if *name == g) {
        return say(sender, "&cA group can't inherit itself.");
    }
    let key = format!("group.{g}");
    let ctx = contexts(&args[if action == "addtemp" { 3 } else { 2 }.min(args.len())..]);
    match action.as_str() {
        "add" | "addtemp" => {
            let mut n = Node::new(&key, true);
            n.contexts = ctx;
            if action == "addtemp" {
                let Some(secs) = args.get(2).and_then(|d| model::parse_duration(d)) else { return say(sender, "&cGive a duration: 1d, 12h, 30m...") };
                n.expiry = Some(model::now() + secs);
            }
            target.edit(|h| {
                h.set(n);
                ((), true)
            });
            say(sender, &format!("&b{label}&a now inherits permissions from &b{g}&a."));
        }
        "remove" => {
            let removed = target.edit(|h| {
                let r = h.unset(&key, &ctx);
                if let Target::User(_) = target
                    && h.parents(model::now()).is_empty()
                {
                    h.set(Node::new("group.default", true));
                }
                (r, r)
            });
            say(sender, &if removed == Some(true) { format!("&b{label}&a no longer inherits permissions from &b{g}&a.") } else { format!("&b{label}&c does not inherit from &b{g}&c.") });
        }
        "set" => {
            target.edit(|h| {
                h.nodes.retain(|n| n.group().is_none());
                h.set(Node::new(&key, true));
                ((), true)
            });
            if let Target::User(uuid) = target {
                crate::with(|s| {
                    if let Some(u) = s.store.users.get_mut(uuid) {
                        u.primary_group = g.clone();
                    }
                    s.store.save_user(uuid);
                });
            }
            say(sender, &format!("&b{label}&a had their existing parent groups cleared, and now only inherits &b{g}&a."));
        }
        _ => say(sender, "&cUsage: parent info|add|remove|set|addtemp"),
    }
}

/// `"&e&lVIP &r"` -> `&e&lVIP &r`: LuckPerms takes a quoted argument as the text inside the quotes.
fn unquote(text: &str) -> String {
    let t = text.trim();
    for q in ['"', '\'']
    {
        if t.len() >= 2 && t.starts_with(q) && t.ends_with(q) {
            return t[1..t.len() - 1].to_owned();
        }
    }
    t.to_owned()
}

/// `... meta info|set|unset|setprefix|setsuffix|removeprefix|removesuffix`
fn meta(sender: &CommandSender, target: &Target, kind: &str, args: &[&str]) {
    let action = args.first().map(|a| a.to_ascii_lowercase()).unwrap_or_else(|| "info".into());
    if !allowed(sender, &format!("luckperms.{kind}.meta.{action}")) {
        return say(sender, "&cYou do not have permission to use this command!");
    }
    let label = target.label();
    match action.as_str() {
        "info" => {
            let lines: Vec<String> = target.nodes().iter().filter(|n| n.is_meta()).map(|n| format!("&f- &3{}", n.describe())).collect();
            say(sender, &format!("&b{label}&a's meta:\n{}", if lines.is_empty() { "&7(none)".to_owned() } else { lines.join("\n") }));
        }
        "setprefix" | "setsuffix" => {
            let which = &action[3..];
            let (weight, text) = match args.get(1).and_then(|w| w.parse::<i32>().ok()) {
                Some(w) => (w, unquote(&args[2.min(args.len())..].join(" "))),
                None => (100, unquote(&args[1.min(args.len())..].join(" "))),
            };
            if text.is_empty() {
                return say(sender, &format!("&cUsage: /lp {kind} <name> meta {action} [weight] <text>"));
            }
            target.edit(|h| {
                h.set(Node::new(&format!("{which}.{weight}.{text}"), true));
                ((), true)
            });
            say(sender, &format!("&b{label}&a had {which} &f\"{text}&f\"&a set at weight &b{weight}&a."));
        }
        "set" => {
            let (Some(key), Some(_)) = (args.get(1), args.get(2)) else {
                return say(sender, &format!("&cUsage: /lp {kind} <name> meta set <key> <value>"));
            };
            let value = unquote(&args[2..].join(" "));
            let lower = key.to_ascii_lowercase();
            target.edit(|h| {
                // One value per key, as LuckPerms' `meta set` leaves it.
                h.nodes.retain(|n| !n.key.strip_prefix("meta.").and_then(crate::model::split_meta).is_some_and(|(k, _)| k.to_ascii_lowercase() == lower));
                h.set(Node::new(&format!("meta.{}.{}", crate::model::escape_meta(key), crate::model::escape_meta(&value)), true));
                ((), true)
            });
            say(sender, &format!("&aSet meta key '&f{key}&a' to '&f{value}&a' for &b{label}&a."));
        }
        "unset" => {
            let Some(key) = args.get(1) else {
                return say(sender, &format!("&cUsage: /lp {kind} <name> meta unset <key>"));
            };
            let lower = key.to_ascii_lowercase();
            let mut gone = false;
            target.edit(|h| {
                let before = h.nodes.len();
                h.nodes.retain(|n| !n.key.strip_prefix("meta.").and_then(crate::model::split_meta).is_some_and(|(k, _)| k.to_ascii_lowercase() == lower));
                gone = h.nodes.len() != before;
                ((), gone)
            });
            if gone {
                say(sender, &format!("&aUnset meta key '&f{key}&a' for &b{label}&a."));
            } else {
                say(sender, &format!("&b{label}&a doesn't have meta key '&f{key}&a' set."));
            }
        }
        "removeprefix" | "removesuffix" => {
            let which = action[6..].to_owned();
            let weight = args.get(1).and_then(|w| w.parse::<i32>().ok());
            target.edit(|h| {
                h.nodes.retain(|n| n.chat_meta(&which).is_none_or(|(w, _)| weight.is_some_and(|x| x != w)));
                ((), true)
            });
            say(sender, &format!("&b{label}&a had their {which}es removed{}.", weight.map_or(String::new(), |w| format!(" at weight {w}"))));
        }
        _ => say(sender, "&cUsage: meta info|set|unset|setprefix|setsuffix|removeprefix|removesuffix"),
    }
}

/// Tab completion: the words, then group, track and player names where they go.
pub fn complete(args: &[&str]) -> Vec<String> {
    let last = args.last().copied().unwrap_or("").to_ascii_lowercase();
    let pick = |words: Vec<String>| words.into_iter().filter(|w| w.starts_with(&last)).collect::<Vec<_>>();
    let groups = || crate::with(|s| s.store.groups.keys().cloned().collect::<Vec<_>>()).unwrap_or_default();
    let tracks = || crate::with(|s| s.store.tracks.keys().cloned().collect::<Vec<_>>()).unwrap_or_default();
    let players = || horizon_bukkit::get_online_players().into_iter().map(|p| p.get_name().to_owned()).collect::<Vec<_>>();
    let words = |w: &[&str]| w.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    match args.len() {
        0 | 1 => pick(words(&["help", "info", "sync", "user", "group", "track", "creategroup", "deletegroup", "listgroups", "createtrack", "deletetrack", "listtracks"])),
        2 => match args[0].to_ascii_lowercase().as_str() {
            "user" => pick(players()),
            "group" | "deletegroup" => pick(groups()),
            "track" | "deletetrack" => pick(tracks()),
            _ => Vec::new(),
        },
        3 => match args[0].to_ascii_lowercase().as_str() {
            "user" => pick(words(&["info", "permission", "parent", "meta", "promote", "demote", "clear"])),
            "group" => pick(words(&["info", "permission", "parent", "meta", "setweight", "listmembers"])),
            "track" => pick(words(&["info", "append", "insert", "remove", "clear"])),
            _ => Vec::new(),
        },
        4 => match args[2].to_ascii_lowercase().as_str() {
            "permission" => pick(words(&["info", "set", "unset", "settemp", "unsettemp", "check"])),
            "parent" => pick(words(&["info", "add", "remove", "set", "addtemp"])),
            "meta" => pick(words(&["info", "set", "unset", "setprefix", "setsuffix", "removeprefix", "removesuffix"])),
            "promote" | "demote" => pick(tracks()),
            _ => Vec::new(),
        },
        5 if args[2].eq_ignore_ascii_case("parent") => pick(groups()),
        _ => Vec::new(),
    }
}
