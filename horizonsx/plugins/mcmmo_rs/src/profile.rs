//! Player data: every profile in memory (mcMMO's FlatFile database, `flatfile/mcmmo.users`, read in full at start
//! on a thread of its own, the same columns so an owner's file carries over), written back on a background
//! thread every `General.Save_Interval` minutes and at stop; leaderboards rebuilt off the server thread.

use crate::skills::{COUNT, Skill};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, RwLock};

/// Ability cooldown columns (seconds since epoch the ability ended), in flatfile order of [`DATS_COLUMNS`].
pub const DATS_COLUMNS: [usize; 13] = [26, 27, 28, 29, 30, 31, 32, 36, 49, 50, 51, 54, 57];
/// Where each [`crate::skills::Ability`] keeps its DATS in that list.
pub fn dats_slot(ab: crate::skills::Ability) -> usize {
    use crate::skills::Ability::*;
    match ab {
        Berserk => 0,
        GigaDrillBreaker => 1,
        TreeFeller => 2,
        GreenTerra => 3,
        SerratedStrikes => 4,
        SkullSplitter => 5,
        SuperBreaker => 6,
        BlastMining => 7,
    }
}

/// (skill, level column, xp column) of the flatfile.
const COLUMNS: [(Skill, usize, usize); 17] = [
    (Skill::Mining, 1, 4),
    (Skill::Woodcutting, 5, 6),
    (Skill::Repair, 7, 15),
    (Skill::Unarmed, 8, 16),
    (Skill::Herbalism, 9, 17),
    (Skill::Excavation, 10, 18),
    (Skill::Archery, 11, 19),
    (Skill::Swords, 12, 20),
    (Skill::Axes, 13, 21),
    (Skill::Acrobatics, 14, 22),
    (Skill::Taming, 24, 25),
    (Skill::Fishing, 34, 35),
    (Skill::Alchemy, 39, 40),
    (Skill::Crossbows, 46, 45),
    (Skill::Tridents, 48, 47),
    (Skill::Maces, 53, 52),
    (Skill::Spears, 56, 55),
];
const FIELDS: usize = 58;

#[derive(Clone, Debug)]
pub struct Profile {
    pub name: String,
    pub uuid: String,
    pub level: [i32; COUNT],
    pub xp: [f32; COUNT],
    pub dats: [i64; 13],
    pub tips: i32,
    pub chimaera: i64,
    pub last_login: i64,
    pub healthbar: String,
    /// Made at a join before the file was read: the file's row wins.
    pub placeholder: bool,
    line: Option<String>,
}

impl Profile {
    pub fn new(uuid: &str, name: &str, starting: i32) -> Profile {
        let mut level = [starting; COUNT];
        for s in crate::skills::ALL {
            if s.is_child() {
                level[s.idx()] = 0;
            }
        }
        Profile { name: name.to_owned(), uuid: uuid.to_ascii_lowercase(), level, xp: [0.0; COUNT], dats: [0; 13], tips: 0, chimaera: 0, last_login: now_secs(), healthbar: "HEARTS".into(), placeholder: false, line: None }
    }

    /// A level as mcMMO reports it: a child skill's is its parents' average.
    pub fn level(&self, skill: Skill) -> i32 {
        if skill.is_child() {
            let parents = skill.parents();
            return parents.iter().map(|p| self.level[p.idx()]).sum::<i32>() / parents.len() as i32;
        }
        self.level[skill.idx()]
    }

    pub fn power_level(&self) -> i32 {
        Skill::non_child().map(|s| self.level[s.idx()]).sum()
    }

    fn parse(line: &str) -> Option<Profile> {
        let f: Vec<&str> = line.split(':').collect();
        if f.len() < 42 || f[0].is_empty() {
            return None;
        }
        let int = |i: usize| f.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        let uuid = f.get(41).copied().unwrap_or("NULL");
        if uuid == "NULL" || uuid.len() < 32 {
            return None;
        }
        let mut p = Profile::new(uuid, f[0], 0);
        for (skill, lc, xc) in COLUMNS {
            p.level[skill.idx()] = int(lc) as i32;
            p.xp[skill.idx()] = int(xc) as f32;
        }
        for (slot, col) in DATS_COLUMNS.iter().enumerate() {
            p.dats[slot] = int(*col) as i64;
        }
        p.tips = int(42) as i32;
        p.chimaera = int(43) as i64;
        p.last_login = int(44) as i64;
        p.healthbar = f.get(38).filter(|v| !v.is_empty() && **v != "IGNORED").unwrap_or(&"HEARTS").to_string();
        p.line = Some(line.to_owned());
        Some(p)
    }

    fn serialize(&self) -> String {
        let mut f: Vec<String> = vec!["IGNORED".to_owned(); FIELDS];
        f[0] = self.name.clone();
        for (skill, lc, xc) in COLUMNS {
            f[lc] = self.level[skill.idx()].to_string();
            f[xc] = (self.xp[skill.idx()] as i32).to_string();
        }
        for (slot, col) in DATS_COLUMNS.iter().enumerate() {
            f[*col] = self.dats[slot].to_string();
        }
        f[38] = self.healthbar.clone();
        f[41] = self.uuid.clone();
        f[42] = self.tips.to_string();
        f[43] = self.chimaera.to_string();
        f[44] = self.last_login.to_string();
        let mut out = f.join(":");
        out.push(':');
        out
    }
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[derive(Default)]
pub struct Store {
    pub by_uuid: HashMap<String, Profile>,
    pub by_name: HashMap<String, String>,
    pub loaded: bool,
    dirty: bool,
}

impl Store {
    pub fn get(&self, uuid: &str) -> Option<&Profile> {
        self.by_uuid.get(uuid)
    }
    /// For a change: the row is written at the next save.
    pub fn get_mut(&mut self, uuid: &str) -> Option<&mut Profile> {
        let p = self.by_uuid.get_mut(uuid)?;
        p.line = None;
        self.dirty = true;
        Some(p)
    }
    pub fn by_name(&self, name: &str) -> Option<&Profile> {
        self.by_name.get(&name.to_ascii_lowercase()).and_then(|u| self.by_uuid.get(u))
    }
    pub fn uuid_of(&self, name: &str) -> Option<String> {
        self.by_name.get(&name.to_ascii_lowercase()).cloned()
    }
    /// The profile of a joining player, made when new (the name kept current).
    pub fn ensure(&mut self, uuid: &str, name: &str, starting: i32) -> &mut Profile {
        let uuid = uuid.to_ascii_lowercase();
        self.dirty = true;
        if !self.by_uuid.contains_key(&uuid) {
            let mut p = Profile::new(&uuid, name, starting);
            p.placeholder = !self.loaded;
            self.by_uuid.insert(uuid.clone(), p);
        }
        let p = self.by_uuid.get_mut(&uuid).expect("just made");
        if p.name != name {
            self.by_name.remove(&p.name.to_ascii_lowercase());
            p.name = name.to_owned();
        }
        self.by_name.insert(name.to_ascii_lowercase(), uuid.clone());
        p.line = None;
        p
    }
    pub fn remove(&mut self, uuid: &str) -> bool {
        match self.by_uuid.remove(uuid) {
            Some(p) => {
                self.by_name.remove(&p.name.to_ascii_lowercase());
                self.dirty = true;
                true
            }
            None => false,
        }
    }
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}

pub static STORE: LazyLock<Mutex<Store>> = LazyLock::new(|| Mutex::new(Store::default()));

pub fn store() -> std::sync::MutexGuard<'static, Store> {
    STORE.lock().unwrap_or_else(|e| e.into_inner())
}

static FOLDER: Mutex<Option<PathBuf>> = Mutex::new(None);
static STOP: AtomicBool = AtomicBool::new(false);
static SAVER: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

fn users_file() -> Option<PathBuf> {
    FOLDER.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|f| f.join("flatfile").join("mcmmo.users"))
}

/// Starts the loader and the saver thread.
pub fn start(folder: PathBuf, on_loaded: fn()) {
    let _ = std::fs::create_dir_all(folder.join("flatfile"));
    *FOLDER.lock().unwrap_or_else(|e| e.into_inner()) = Some(folder);
    STOP.store(false, Ordering::SeqCst);
    let handle = std::thread::Builder::new()
        .name("mcMMO-data".into())
        .spawn(move || {
            load_all();
            on_loaded();
            let mut since_save = std::time::Instant::now();
            let mut since_board = std::time::Instant::now() - std::time::Duration::from_secs(3600);
            loop {
                for _ in 0..10 {
                    if STOP.load(Ordering::SeqCst) {
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                let s = crate::settings::get();
                if since_save.elapsed().as_secs() >= s.save_minutes * 60 {
                    save();
                    crate::placed::save_dirty();
                    crate::party::save();
                    since_save = std::time::Instant::now();
                }
                if since_board.elapsed().as_secs() >= s.leaderboard_refresh_secs || BOARD_NOW.swap(false, Ordering::SeqCst) {
                    rebuild_leaderboards();
                    since_board = std::time::Instant::now();
                }
            }
        })
        .ok();
    *SAVER.lock().unwrap_or_else(|e| e.into_inner()) = handle;
}

static BOARD_NOW: AtomicBool = AtomicBool::new(false);

/// Leaderboards rebuilt soon (after an admin edit).
pub fn refresh_boards() {
    BOARD_NOW.store(true, Ordering::SeqCst);
}

/// Stops the saver and writes everything (on_disable).
pub fn stop() {
    STOP.store(true, Ordering::SeqCst);
    if let Some(h) = SAVER.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = h.join();
    }
    save();
}

fn load_all() {
    let Some(path) = users_file() else { return };
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut rows: Vec<Profile> = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if let Some(p) = Profile::parse(line) {
            rows.push(p);
        }
    }
    let mut st = store();
    let mut by_uuid: HashMap<String, Profile> = HashMap::with_capacity(rows.len() + st.by_uuid.len());
    let mut by_name = HashMap::with_capacity(rows.len());
    for p in rows {
        by_name.insert(p.name.to_ascii_lowercase(), p.uuid.clone());
        by_uuid.insert(p.uuid.clone(), p);
    }
    // Players who joined while the file was read keep their name; their stored row wins over the blank one.
    for (uuid, p) in std::mem::take(&mut st.by_uuid) {
        match by_uuid.get_mut(&uuid) {
            Some(stored) if p.placeholder => {
                by_name.remove(&stored.name.to_ascii_lowercase());
                stored.name = p.name.clone();
                stored.line = None;
                by_name.insert(p.name.to_ascii_lowercase(), uuid);
            }
            _ => {
                by_name.insert(p.name.to_ascii_lowercase(), uuid.clone());
                by_uuid.insert(uuid, p);
            }
        }
    }
    st.by_uuid = by_uuid;
    st.by_name = by_name;
    st.loaded = true;
    drop(st);
    rebuild_leaderboards();
}

/// The file written (the lock held only while the rows are made).
pub fn save() {
    let Some(path) = users_file() else { return };
    let text = {
        let mut st = store();
        if !st.dirty || !st.loaded {
            return;
        }
        st.dirty = false;
        let mut out = String::with_capacity(st.by_uuid.len() * 220);
        out.push_str("# mcMMO Database (written by the Horizon mcMMO)\n");
        for p in st.by_uuid.values_mut() {
            if p.line.is_none() {
                p.line = Some(p.serialize());
            }
            out.push_str(p.line.as_deref().unwrap_or_default());
            out.push('\n');
        }
        out
    };
    let tmp = path.with_extension("users.tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// One skill's (or the power level's) ranking: names and levels, best first, and each uuid's place.
#[derive(Default)]
pub struct Board {
    pub rows: Vec<(String, String, i32)>,
    pub place: HashMap<String, usize>,
}

#[derive(Default)]
pub struct Boards {
    pub skills: Vec<Board>,
    pub power: Board,
}

static BOARDS: LazyLock<RwLock<Arc<Boards>>> = LazyLock::new(|| RwLock::new(Arc::new(Boards::default())));

pub fn boards() -> Arc<Boards> {
    BOARDS.read().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn rebuild_leaderboards() {
    let snapshot: Vec<(String, String, [i32; COUNT])> = {
        let st = store();
        st.by_uuid.values().map(|p| (p.uuid.clone(), p.name.clone(), p.level)).collect()
    };
    let make = |key: &dyn Fn(&[i32; COUNT]) -> i32| {
        let mut rows: Vec<(String, String, i32)> = snapshot.iter().map(|(u, n, l)| (u.clone(), n.clone(), key(l))).filter(|r| r.2 > 0).collect();
        rows.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.1.to_ascii_lowercase().cmp(&b.1.to_ascii_lowercase())));
        let place = rows.iter().enumerate().map(|(i, r)| (r.0.clone(), i + 1)).collect();
        Board { rows, place }
    };
    let mut skills = Vec::with_capacity(COUNT);
    for s in crate::skills::ALL {
        if s.is_child() {
            skills.push(Board::default());
        } else {
            let i = s.idx();
            skills.push(make(&move |l: &[i32; COUNT]| l[i]));
        }
    }
    let power = make(&|l: &[i32; COUNT]| Skill::non_child().map(|s| l[s.idx()]).sum());
    *BOARDS.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(Boards { skills, power });
}

#[cfg(test)]
mod tests {
    #[test]
    fn row_round_trip() {
        let mut p = super::Profile::new("0b8e3c9a-1111-2222-3333-444455556666", "Steve", 0);
        p.level[crate::skills::Skill::Mining.idx()] = 120;
        p.xp[crate::skills::Skill::Spears.idx()] = 55.0;
        let line = p.serialize();
        let q = super::Profile::parse(&line).unwrap();
        assert_eq!(q.level[crate::skills::Skill::Mining.idx()], 120);
        assert_eq!(q.xp[crate::skills::Skill::Spears.idx()], 55.0);
        assert_eq!(q.name, "Steve");
    }
}
