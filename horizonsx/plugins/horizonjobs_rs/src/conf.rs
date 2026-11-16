//! config.yml: read at enable and on `/jobs admin reload`, kept as plain values the hot paths read.

use horizon_kit::{app, yaml};
use serde_yaml::Value as Yaml;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub const DEFAULT: &str = include_str!("../data/config.yml");

/// What a job pays for: the kinds of work.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Work {
    Break,
    Kill,
    Fish,
    Breed,
    Harvest,
}

impl Work {
    pub const ALL: [Work; 5] = [Work::Break, Work::Kill, Work::Fish, Work::Breed, Work::Harvest];
    pub fn key(self) -> &'static str {
        match self {
            Work::Break => "break",
            Work::Kill => "kill",
            Work::Fish => "fish",
            Work::Breed => "breed",
            Work::Harvest => "harvest",
        }
    }
    pub fn verb(self) -> &'static str {
        match self {
            Work::Break => "Break",
            Work::Kill => "Kill",
            Work::Fish => "Catch",
            Work::Breed => "Breed",
            Work::Harvest => "Harvest",
        }
    }
}

/// One payout line: the things it covers, money and exp each.
#[derive(Clone, Debug)]
pub struct Pay {
    pub work: Work,
    pub things: Vec<String>,
    pub money: f64,
    pub exp: f64,
}

#[derive(Clone, Debug)]
pub struct Job {
    /// Lower-case id (`miner`).
    pub id: String,
    pub name: String,
    pub plural: String,
    pub color: String,
    pub light: String,
    pub icon: String,
    pub description: Vec<String>,
    pub base_exp: f64,
    pub exp_step: f64,
    pub max_level: u32,
    /// Payout lines in config order (the payouts menu).
    pub pays: Vec<Pay>,
    /// (work, thing) -> (money, exp): the hot lookup.
    pub table: HashMap<(Work, String), (f64, f64)>,
}

impl Job {
    /// Exp needed to go from `level` to the next.
    pub fn needed(&self, level: u32) -> f64 {
        (self.base_exp * (1.0 + self.exp_step * f64::from(level.max(1) - 1))).round()
    }
    pub fn pays_for(&self, work: Work, thing: &str) -> Option<(f64, f64)> {
        self.table.get(&(work, thing.to_owned())).copied()
    }
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub max_jobs: usize,
    pub keep_progress: bool,
    pub join_cooldown_ms: u64,
    pub pay_secs: u64,
    pub income_per_level: f64,
    pub spawner_multiplier: f64,
    pub boost_percent: f64,
    pub creative_earns: bool,
    pub level_money: f64,
    pub announce_every: u32,
    pub jobs: Vec<Job>,
}

impl Settings {
    pub fn job(&self, id: &str) -> Option<&Job> {
        let id = id.to_ascii_lowercase();
        self.jobs.iter().find(|j| j.id == id || j.name.to_ascii_lowercase() == id || j.plural.to_ascii_lowercase() == id)
    }
    /// Every (work, thing) any job pays for: the blocks worth guarding against place-and-break.
    pub fn paid(&self, work: Work, thing: &str) -> bool {
        let k = (work, thing.to_owned());
        self.jobs.iter().any(|j| j.table.contains_key(&k))
    }
}

fn read_job(id: &str, n: &Yaml) -> Job {
    let name = yaml::string(n, "name").unwrap_or_else(|| {
        let mut c = id.chars();
        c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
    });
    let mut pays = Vec::new();
    let mut table = HashMap::new();
    for work in Work::ALL {
        let Some(Yaml::Mapping(m)) = n.get(work.key()) else { continue };
        for (k, v) in m {
            let Some(key) = yaml::key_str(k) else { continue };
            let nums: Vec<f64> = match v {
                Yaml::Sequence(s) => s.iter().filter_map(|x| x.as_f64().or_else(|| x.as_str().and_then(|t| t.trim().parse().ok()))).collect(),
                Yaml::Mapping(_) => vec![yaml::num(v, "money", 0.0), yaml::num(v, "exp", 0.0)],
                other => other.as_f64().map(|f| vec![f, 0.0]).unwrap_or_default(),
            };
            let (money, exp) = (nums.first().copied().unwrap_or(0.0), nums.get(1).copied().unwrap_or(0.0));
            let things: Vec<String> = key.split(',').map(|t| t.trim().trim_start_matches("minecraft:").to_ascii_lowercase()).filter(|t| !t.is_empty()).collect();
            for t in &things {
                table.insert((work, t.clone()), (money, exp));
            }
            if !things.is_empty() {
                pays.push(Pay { work, things, money, exp });
            }
        }
    }
    Job {
        id: id.to_ascii_lowercase(),
        plural: yaml::string(n, "plural").unwrap_or_else(|| format!("{name}s")),
        color: yaml::string(n, "color").unwrap_or_else(|| "#a1a1aa".into()),
        light: yaml::string(n, "light").unwrap_or_else(|| "#e4e4e7".into()),
        icon: yaml::string(n, "icon").unwrap_or_else(|| "PAPER".into()),
        description: yaml::list_or_one(n, "description"),
        base_exp: yaml::num(n, "base-exp", 500.0).max(1.0),
        exp_step: yaml::num(n, "exp-step", 0.35).max(0.0),
        max_level: yaml::int(n, "max-level", 100).clamp(1, 10_000) as u32,
        name,
        pays,
        table,
    }
}

pub fn parse(text: &str) -> Result<Settings, String> {
    let doc = yaml::parse(text)?;
    let mut jobs = Vec::new();
    if let Some(Yaml::Mapping(m)) = doc.get("jobs") {
        for (k, v) in m {
            if let Some(id) = yaml::key_str(k) {
                jobs.push(read_job(&id, v));
            }
        }
    }
    if jobs.is_empty() {
        return Err("no jobs in config.yml".into());
    }
    Ok(Settings {
        max_jobs: yaml::int(&doc, "max-jobs", 1).clamp(1, 64) as usize,
        keep_progress: yaml::boolean(&doc, "keep-progress-on-leave", true),
        join_cooldown_ms: (yaml::num(&doc, "join-cooldown-minutes", 5.0).max(0.0) * 60_000.0) as u64,
        pay_secs: yaml::int(&doc, "pay-every-seconds", 3).clamp(1, 600) as u64,
        income_per_level: yaml::num(&doc, "income-per-level", 0.02).max(0.0),
        spawner_multiplier: yaml::num(&doc, "spawner-multiplier", 0.25).clamp(0.0, 1.0),
        boost_percent: yaml::num(&doc, "boost-percent", 0.0),
        creative_earns: yaml::boolean(&doc, "creative-earns", false),
        level_money: yaml::num(&doc, "level-up.money-per-level", 50.0).max(0.0),
        announce_every: yaml::int(&doc, "level-up.announce-every", 10).max(0) as u32,
        jobs,
    })
}

static SETTINGS: RwLock<Option<Arc<Settings>>> = RwLock::new(None);

pub fn get() -> Arc<Settings> {
    if let Some(s) = SETTINGS.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return s.clone();
    }
    let s = Arc::new(parse(DEFAULT).expect("built-in config.yml"));
    *SETTINGS.write().unwrap_or_else(|e| e.into_inner()) = Some(s.clone());
    s
}

/// Reads plugins/HorizonJobs/config.yml (written from the default the first time). A broken file keeps what was
/// loaded before and says why.
pub fn reload() -> Result<usize, String> {
    let path = app::folder().join("config.yml");
    if !path.exists() {
        let _ = std::fs::create_dir_all(app::folder());
        let _ = std::fs::write(&path, DEFAULT);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("config.yml: {e}"))?;
    let s = parse(&text).map_err(|e| format!("config.yml: {e}"))?;
    let n = s.jobs.len();
    *SETTINGS.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(s));
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_config() {
        let s = parse(DEFAULT).unwrap();
        assert_eq!(s.jobs.iter().map(|j| j.id.as_str()).collect::<Vec<_>>(), ["miner", "lumberjack", "farmer", "fisher", "hunter"]);
        let miner = s.job("miner").unwrap();
        assert_eq!(miner.needed(1), 1100.0);
        assert_eq!(miner.pays_for(Work::Break, "deepslate_diamond_ore"), Some((12.0, 60.0)));
        assert_eq!(s.job("Hunters").unwrap().needed(1), 400.0);
        assert_eq!(s.job("lumberjack").unwrap().needed(1), 580.0);
        assert_eq!(s.job("farmer").unwrap().needed(1), 1144.0);
        assert_eq!(s.job("fisher").unwrap().needed(1), 220.0);
        assert!(s.job("farmer").unwrap().pays_for(Work::Breed, "cow").is_some());
        assert!(s.paid(Work::Break, "oak_log"));
        assert_eq!(s.max_jobs, 1);
    }
}
