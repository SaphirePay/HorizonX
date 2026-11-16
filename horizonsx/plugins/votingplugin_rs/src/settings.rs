//! Config.yml (with its Format messages), VoteSites.yml, SpecialRewards.yml, Shop.yml, Rewards/*.yml and the
//! receiver's Votifier.yml, read once into tables.

use horizon_kit::Config;
use serde_yaml::Value as Yaml;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock, RwLock};

const CONFIG: &str = include_str!("../data/Config.yml");
const SITES: &str = include_str!("../data/VoteSites.yml");
const SPECIAL: &str = include_str!("../data/SpecialRewards.yml");
const SHOP: &str = include_str!("../data/Shop.yml");
const GUI: &str = include_str!("../data/GUI.yml");
const REWARD_EXAMPLES: [(&str, &str); 2] = [("ExampleBasic.yml", include_str!("../data/Rewards/ExampleBasic.yml")), ("ExampleAdvanced.yml", include_str!("../data/Rewards/ExampleAdvanced.yml"))];
pub const VOTIFIER: &str = "# The Votifier receiver built into the Horizon VotingPlugin (NuVotifier compatible: protocol v1 with RSA\n\
# keys, v2 with tokens). Keys are read from plugins/Votifier/rsa when that folder exists, else from rsa/ here\n\
# (made on first start). Give vote sites this server's address, the port below and the public key or token.\n\
Enabled: true\n\
Host: 0.0.0.0\n\
Port: 8192\n\
# v2 tokens by service name; 'default' is used for any other service.\n\
Tokens:\n\
  default: ''\n\
Debug: false\n";

#[derive(Clone)]
pub struct Site {
    pub key: String,
    pub name: String,
    pub service: String,
    pub url: String,
    pub enabled: bool,
    pub hidden: bool,
    pub priority: i64,
    pub delay_ms: i64,
    pub daily: bool,
    pub force_offline: bool,
    pub item: Yaml,
    pub rewards: Yaml,
}

#[derive(Clone)]
pub struct Milestone {
    pub name: String,
    pub total: String,
    pub at: Vec<i64>,
    pub every: i64,
    pub limit: Option<(String, i64)>,
    pub rewards: Yaml,
}

#[derive(Clone)]
pub struct Streak {
    pub name: String,
    pub kind: String,
    pub amount: i64,
    pub recurring: bool,
    pub rewards: Yaml,
}

pub struct Settings {
    pub cfg: Config,
    pub fmt: HashMap<String, String>,
    pub lists: HashMap<String, Vec<String>>,
    pub sites: Vec<Site>,
    pub every_site: Yaml,
    pub auto_create: bool,
    pub points_on_vote: i64,
    pub broadcast_type: String,
    pub broadcast_msg: String,
    pub milestones: Vec<Milestone>,
    pub cumulative: Vec<(i64, String, Yaml)>,
    pub old_milestones: Vec<(i64, Yaml)>,
    pub first_vote: Yaml,
    pub first_vote_today: Yaml,
    pub all_sites: Yaml,
    pub party: Yaml,
    pub streaks: Vec<Streak>,
    pub shop: Yaml,
    pub gui: Yaml,
    pub reward_files: HashMap<String, Yaml>,
    pub reminders: Yaml,
    pub top_default: String,
    pub votifier: Config,
    pub offline_limit: i64,
}

static CURRENT: LazyLock<RwLock<Arc<Settings>>> = LazyLock::new(|| RwLock::new(Arc::new(build(Path::new("")))));

pub fn get() -> Arc<Settings> {
    CURRENT.read().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn load(folder: &Path) {
    let s = build(folder);
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(s);
}

fn file(folder: &Path, name: &str, default: &str) -> Config {
    if folder.as_os_str().is_empty() {
        return Config::parse(default);
    }
    Config::load(folder, name, default)
}

fn flatten(v: &Yaml, prefix: &str, out: &mut HashMap<String, String>, lists: &mut HashMap<String, Vec<String>>) {
    match v {
        Yaml::Mapping(m) => {
            for (k, v) in m {
                let Some(k) = k.as_str() else { continue };
                let p = if prefix.is_empty() { k.to_owned() } else { format!("{prefix}.{k}") };
                flatten(v, &p, out, lists);
            }
        }
        Yaml::String(s) => {
            out.insert(prefix.to_owned(), s.clone());
        }
        Yaml::Sequence(items) => {
            let l: Vec<String> = items.iter().filter_map(|i| i.as_str().map(str::to_owned)).collect();
            out.insert(prefix.to_owned(), l.join("\n"));
            lists.insert(prefix.to_owned(), l);
        }
        Yaml::Number(n) => {
            out.insert(prefix.to_owned(), n.to_string());
        }
        Yaml::Bool(b) => {
            out.insert(prefix.to_owned(), b.to_string());
        }
        _ => {}
    }
}

/// `24h`, `30m`, `1d`, a bare number of hours (VoteDelay's old form).
pub fn duration_ms(v: Option<&Yaml>, bare_unit_ms: i64) -> i64 {
    match v {
        Some(Yaml::Number(n)) => (n.as_f64().unwrap_or(0.0) * bare_unit_ms as f64) as i64,
        Some(Yaml::String(s)) => match s.trim().parse::<f64>() {
            Ok(n) => (n * bare_unit_ms as f64) as i64,
            Err(_) => horizon_kit::time::parse(s).map(|m| m as i64).unwrap_or(0),
        },
        _ => 0,
    }
}

/// `At: [5, "10..50 step 10", 100]` and `At: 1`.
fn at_list(v: Option<&Yaml>) -> Vec<i64> {
    let mut out = Vec::new();
    let mut one = |item: &Yaml| match item {
        Yaml::Number(n) => out.push(n.as_i64().unwrap_or(0)),
        Yaml::String(s) => {
            let s = s.trim();
            if let Some((range, step)) = s.split_once("step") {
                let step: i64 = step.trim().parse().unwrap_or(1).max(1);
                if let Some((a, b)) = range.trim().split_once("..") {
                    let (a, b): (i64, i64) = (a.trim().parse().unwrap_or(0), b.trim().parse().unwrap_or(0));
                    let mut x = a;
                    while x <= b {
                        out.push(x);
                        x += step;
                    }
                }
            } else if let Some((a, b)) = s.split_once("..") {
                let (a, b): (i64, i64) = (a.trim().parse().unwrap_or(0), b.trim().parse().unwrap_or(0));
                out.extend(a..=b);
            } else if let Ok(n) = s.parse() {
                out.push(n);
            }
        }
        _ => {}
    };
    match v {
        Some(Yaml::Sequence(items)) => items.iter().for_each(&mut one),
        Some(other) => one(other),
        None => {}
    }
    out
}

fn build(folder: &Path) -> Settings {
    let cfg = file(folder, "Config.yml", CONFIG);
    let sites_cfg = file(folder, "VoteSites.yml", SITES);
    let special = file(folder, "SpecialRewards.yml", SPECIAL);
    let shop = file(folder, "Shop.yml", SHOP);
    let gui = file(folder, "GUI.yml", GUI);
    let votifier = file(folder, "Votifier.yml", VOTIFIER);
    let mut reward_files = HashMap::new();
    if !folder.as_os_str().is_empty() {
        let dir = folder.join("Rewards");
        if !dir.exists() {
            let _ = std::fs::create_dir_all(&dir);
            for (n, t) in REWARD_EXAMPLES {
                let _ = std::fs::write(dir.join(n), t);
            }
        }
        for f in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = f.path();
            if p.extension().is_some_and(|e| e == "yml")
                && let Some(doc) = std::fs::read_to_string(&p).ok().and_then(|t| horizon_kit::yaml::parse(&t).ok())
            {
                reward_files.insert(p.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default(), doc);
            }
        }
    }
    let mut fmt = HashMap::new();
    let mut lists = HashMap::new();
    if let Some(f) = cfg.at("Format") {
        flatten(f, "Format", &mut fmt, &mut lists);
    }
    let mut sites = Vec::new();
    if let Some(Yaml::Mapping(m)) = sites_cfg.at("VoteSites") {
        for (k, v) in m {
            let Some(key) = k.as_str() else { continue };
            let s = |f: &str, d: &str| v.get(f).and_then(|x| x.as_str()).unwrap_or(d).to_owned();
            sites.push(Site {
                key: key.to_owned(),
                name: s("Name", key),
                service: s("ServiceSite", key),
                url: s("VoteURL", ""),
                enabled: v.get("Enabled").and_then(|x| x.as_bool()).unwrap_or(true),
                hidden: v.get("Hidden").and_then(|x| x.as_bool()).unwrap_or(false),
                priority: v.get("Priority").and_then(|x| x.as_i64()).unwrap_or(0),
                delay_ms: duration_ms(v.get("VoteDelay"), 3_600_000).max(0),
                daily: v.get("VoteDelayDaily").and_then(|x| x.as_bool()).unwrap_or(false),
                force_offline: v.get("ForceOffline").and_then(|x| x.as_bool()).unwrap_or(false),
                item: v.get("DisplayItem").cloned().unwrap_or(Yaml::Null),
                rewards: v.get("Rewards").cloned().unwrap_or(Yaml::Null),
            });
        }
    }
    sites.sort_by(|a, b| b.priority.cmp(&a.priority));
    let mut milestones = Vec::new();
    if let Some(Yaml::Mapping(m)) = special.at("VoteMilestones") {
        for (k, v) in m {
            let Some(name) = k.as_str() else { continue };
            if !v.get("Enabled").and_then(|x| x.as_bool()).unwrap_or(true) {
                continue;
            }
            milestones.push(Milestone {
                name: name.to_owned(),
                total: v.get("Total").and_then(|x| x.as_str()).unwrap_or("AllTime").to_ascii_uppercase().replace("_VOTES", ""),
                at: at_list(v.get("At")),
                every: v.get("Every").and_then(|x| x.as_i64()).unwrap_or(0),
                limit: v.get("Limit").map(|l| (l.get("Type").and_then(|x| x.as_str()).unwrap_or("").to_ascii_uppercase(), duration_ms(l.get("Duration"), 1000))),
                rewards: v.get("Rewards").cloned().unwrap_or(Yaml::Null),
            });
        }
    }
    let mut cumulative = Vec::new();
    if let Some(Yaml::Mapping(m)) = special.at("Cumulative") {
        for (k, v) in m {
            let n = k.as_i64().or_else(|| k.as_str().and_then(|s| s.parse().ok())).unwrap_or(0);
            if v.get("Enabled").and_then(|x| x.as_bool()).unwrap_or(false) && n > 0 {
                cumulative.push((n, v.get("TotalToUse").and_then(|x| x.as_str()).unwrap_or("AllTime").to_ascii_uppercase(), v.get("Rewards").cloned().unwrap_or(Yaml::Null)));
            }
        }
    }
    let mut old_milestones = Vec::new();
    if let Some(Yaml::Mapping(m)) = special.at("MileStones") {
        for (k, v) in m {
            let n = k.as_i64().or_else(|| k.as_str().and_then(|s| s.parse().ok())).unwrap_or(0);
            if v.get("Enabled").and_then(|x| x.as_bool()).unwrap_or(false) && n > 0 {
                old_milestones.push((n, v.get("Rewards").cloned().unwrap_or(Yaml::Null)));
            }
        }
    }
    let mut streaks = Vec::new();
    if let Some(Yaml::Mapping(m)) = special.at("VoteStreaks") {
        for (k, v) in m {
            let Some(name) = k.as_str() else { continue };
            if name == "ProgressGroups" || !v.get("Enabled").and_then(|x| x.as_bool()).unwrap_or(false) {
                continue;
            }
            streaks.push(Streak {
                name: name.to_owned(),
                kind: v.get("Type").and_then(|x| x.as_str()).unwrap_or("DAILY").to_ascii_uppercase(),
                amount: v.get("Requirements").and_then(|r| r.get("Amount")).and_then(|x| x.as_i64()).unwrap_or(1),
                recurring: v.get("Recurring").and_then(|x| x.as_bool()).unwrap_or(false),
                rewards: v.get("Rewards").cloned().unwrap_or(Yaml::Null),
            });
        }
    }
    let y = |c: &Config, p: &str| c.at(p).cloned().unwrap_or(Yaml::Null);
    Settings {
        auto_create: cfg.bool("AutoCreateVoteSites", true),
        points_on_vote: cfg.int("PointsOnVote", 1),
        broadcast_type: cfg.str("VoteBroadcast.Type", "EVERY_VOTE").to_ascii_uppercase(),
        broadcast_msg: cfg.str("VoteBroadcast.Format.BroadcastMsg", "&6[&4Vote&6] &aThanks &e%player% &afor voting on &e%site%&a!"),
        reminders: y(&cfg, "VoteReminders"),
        top_default: cfg.str("VoteTopDefault", "Monthly"),
        offline_limit: cfg.int("OfflineVotesLimit.Limit", -1),
        every_site: y(&sites_cfg, "EverySiteReward"),
        first_vote: y(&special, "FirstVote"),
        first_vote_today: y(&special, "FirstVoteToday"),
        all_sites: y(&special, "AllSites"),
        party: y(&special, "VoteParty"),
        shop: shop.doc.clone(),
        gui: gui.doc.clone(),
        cfg,
        fmt,
        lists,
        sites,
        milestones,
        cumulative,
        old_milestones,
        streaks,
        reward_files,
        votifier,
    }
}

impl Settings {
    /// A Format message (`Format.Commands.Vote.Sites`) with `%key%` filled (any case), coloured.
    pub fn msg(&self, key: &str, vars: &[(&str, &str)]) -> String {
        let raw = self.fmt.get(key).cloned().unwrap_or_default();
        colour(&fill(&raw, vars))
    }
    pub fn lines(&self, key: &str, vars: &[(&str, &str)]) -> Vec<String> {
        match self.lists.get(key) {
            Some(l) => l.iter().map(|x| colour(&fill(x, vars))).collect(),
            None => self.fmt.get(key).map(|x| vec![colour(&fill(x, vars))]).unwrap_or_default(),
        }
    }
    pub fn site_by_service(&self, service: &str) -> Option<&Site> {
        self.sites.iter().find(|s| s.service.eq_ignore_ascii_case(service) || s.key.eq_ignore_ascii_case(service) || s.name.eq_ignore_ascii_case(service))
    }
    pub fn site(&self, name: &str) -> Option<&Site> {
        self.sites.iter().find(|s| s.key.eq_ignore_ascii_case(name) || s.name.eq_ignore_ascii_case(name) || s.service.eq_ignore_ascii_case(name))
    }
    pub fn enabled_sites(&self) -> impl Iterator<Item = &Site> {
        self.sites.iter().filter(|s| s.enabled)
    }
}

/// `%key%` (any case) -> value.
pub fn fill(text: &str, vars: &[(&str, &str)]) -> String {
    if !text.contains('%') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('%') {
            Some(j) => {
                let key = &after[..j];
                match vars.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
                    Some((_, v)) => {
                        out.push_str(v);
                        rest = &after[j + 1..];
                    }
                    None => {
                        out.push('%');
                        rest = after;
                    }
                }
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn colour(text: &str) -> String {
    horizon_kit::text::colorize(text)
}

#[cfg(test)]
mod tests {
    #[test]
    fn loads() {
        let s = super::get();
        assert!(!s.sites.is_empty());
        assert!(s.fmt.contains_key("Format.Commands.Vote.Sites"));
        assert_eq!(super::fill("Hi %Player%, %x%", &[("player", "Bo")]), "Hi Bo, %x%");
        assert_eq!(super::at_list(Some(&horizon_kit::yaml::parse("[5, '10..30 step 10', 100]").unwrap())), vec![5, 10, 20, 30, 100]);
        assert!(s.milestones.iter().any(|m| m.name == "FirstVoteAllTime"));
    }
}
