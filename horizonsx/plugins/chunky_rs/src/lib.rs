//! Chunky 1.5.3 for Horizon, written again in Rust (the Java it follows is in java/). Same `/chunky` (`/cy`),
//! permissions, `config.yml`, `tasks/<world>.properties` (pause/continue/continue-on-restart), shapes and
//! patterns, and `trim`.
//!
//! How it generates: the engine makes chunks off the server thread when a chunk gets a force-load ticket. A task
//! keeps up to 50 tickets out; once a tick it checks which of them are loaded (one cheap query each), lets those
//! go (the engine saves and unloads them) and hands out new ones. Chunks already on disk are skipped by reading
//! each region file's header once (no game query per chunk); shapes are plain maths. The server thread does a
//! few dozen queries a tick at most; the generation itself is the engine's.

mod iter;
mod shape;
mod trim;

use horizon_bukkit::{CommandSender, Plugin, World, bukkit_plugin};
use horizon_kit::Config;
use iter::ChunkIter;
use shape::Shape;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

const LANG: &str = include_str!("../data/lang/en.json");
const DEFAULT_CONFIG: &str = include_str!("../data/config.yml");
/// `chunky.maxWorkingCount`: tickets out at once per task.
const MAX_WORKING: usize = 50;
/// Iterator steps a task may take in one tick (chunks outside the shape or already made cost only maths).
const STEPS_PER_TICK: usize = 4096;

static PLUGIN: Mutex<Option<Plugin>> = Mutex::new(None);
static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::default()));
static TRANSLATIONS: LazyLock<HashMap<String, String>> = LazyLock::new(|| serde_json::from_str(LANG).unwrap_or_default());

fn plugin() -> Option<Plugin> {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner())
}

/// `Translator.translateKey`: the English text with `%s` filled, `&` colours, `[Chunky] ` in front when prefixed.
pub fn tr(key: &str, prefixed: bool, args: &[String]) -> String {
    let mut text = TRANSLATIONS.get(key).cloned().unwrap_or_else(|| key.to_owned());
    let mut from = 0;
    for a in args {
        if let Some(at) = text[from..].find("%s").map(|i| i + from) {
            text.replace_range(at..at + 2, a);
            from = at + a.len();
        }
    }
    let text = text.replace("%%", "%");
    let text = if prefixed { format!("[Chunky] {text}") } else { text };
    horizon_bukkit::chat_color::translate_alternate_color_codes('&', &text)
}

fn say(sender: &CommandSender, key: &str, args: &[String]) {
    sender.send_message(&tr(key, true, args));
}

fn console(key: &str, args: &[String]) {
    horizon_bukkit::get_console_sender().send_message(&tr(key, true, args));
}

/// `Formatting.number`: up to two decimals, no trailing zeros.
pub fn num(n: f64) -> String {
    let s = format!("{n:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".to_owned() } else { s.to_owned() }
}

// ---- Selection ----

#[derive(Clone, Debug)]
pub struct Selection {
    pub world: String,
    pub center_x: f64,
    pub center_z: f64,
    pub radius_x: f64,
    pub radius_z: f64,
    pub pattern: String,
    pub pattern_value: Option<String>,
    pub shape: String,
}

impl Default for Selection {
    fn default() -> Self {
        let world = horizon_bukkit::get_worlds().first().map(|w| w.get_name().to_owned()).unwrap_or_else(|| "world".into());
        Self { world, center_x: 0.0, center_z: 0.0, radius_x: 500.0, radius_z: 500.0, pattern: "region".into(), pattern_value: None, shape: "square".into() }
    }
}

impl Selection {
    pub fn center_chunk(&self) -> (i32, i32) {
        ((self.center_x as i32) >> 4, (self.center_z as i32) >> 4)
    }
    pub fn radius_chunks(&self) -> (i32, i32) {
        ((self.radius_x / 16.0).ceil() as i32, (self.radius_z / 16.0).ceil() as i32)
    }
    pub fn uses_z(&self) -> bool {
        matches!(self.shape.as_str(), "rectangle" | "ellipse" | "oval")
    }
    pub fn radius_text(&self) -> String {
        if self.uses_z() { format!("{}, {}", num(self.radius_x), num(self.radius_z)) } else { num(self.radius_x) }
    }
}

// ---- Tasks ----

pub struct Task {
    sel: Selection,
    shape: Shape,
    iter: Box<dyn ChunkIter>,
    region_dir: PathBuf,
    /// Region (x, z) -> its 1024 chunk locations (0 = no chunk on disk), read once.
    headers: HashMap<(i32, i32), Option<Box<[u32; 1024]>>>,
    in_flight: Vec<(i32, i32)>,
    done: u64,
    started: Instant,
    prev_ms: u64,
    exhausted: bool,
    cancelled: bool,
    last: (i32, i32),
    samples: VecDeque<(Instant, u64)>,
    last_update: Option<Instant>,
}

impl Task {
    fn new(sel: Selection, done: u64, prev_ms: u64) -> Option<Task> {
        let world = horizon_bukkit::get_world(&sel.world)?;
        let region_dir = region_dir(&world, "region");
        let mut iter = iter::create(&sel, &region_dir, &data_dir());
        // Resuming: the iterator walks past what was done (maths only).
        for _ in 0..done {
            if iter.next().is_none() {
                break;
            }
        }
        Some(Task {
            shape: Shape::new(&sel),
            sel,
            iter,
            region_dir,
            headers: HashMap::new(),
            in_flight: Vec::new(),
            done,
            started: Instant::now(),
            prev_ms,
            exhausted: false,
            cancelled: false,
            last: (0, 0),
            samples: VecDeque::new(),
            last_update: None,
        })
    }

    fn total(&self) -> u64 {
        self.iter.total().max(1)
    }

    fn total_ms(&self) -> u64 {
        self.prev_ms + self.started.elapsed().as_millis() as u64
    }

    /// The chunk is on disk already (its region file's header says so).
    fn on_disk(&mut self, x: i32, z: i32) -> bool {
        let key = (x >> 5, z >> 5);
        let dir = &self.region_dir;
        let header = self.headers.entry(key).or_insert_with(|| trim::read_header(&dir.join(format!("r.{}.{}.mca", key.0, key.1))));
        header.as_ref().is_some_and(|h| h[((x & 31) + (z & 31) * 32) as usize] != 0)
    }

    fn counted(&mut self, x: i32, z: i32, made: bool) {
        self.done += 1;
        self.last = (x, z);
        if made {
            let now = Instant::now();
            match self.samples.back_mut() {
                Some((at, n)) if now.duration_since(*at).as_millis() < 1000 => *n += 1,
                _ => self.samples.push_back((now, 1)),
            }
            while self.samples.front().is_some_and(|(at, _)| now.duration_since(*at).as_secs() > 30) {
                self.samples.pop_front();
            }
        }
    }

    fn rate(&self) -> f64 {
        let Some((first, _)) = self.samples.front() else { return 0.0 };
        let secs = first.elapsed().as_secs_f64();
        let n: u64 = self.samples.iter().map(|(_, n)| n).sum();
        if secs > 0.1 { n as f64 / secs } else { 0.0 }
    }

    fn update_line(&self, complete: bool) -> String {
        let percent = format!("{:.2}", (100.0 * self.done as f64 / self.total() as f64).min(100.0));
        let secs = if complete {
            self.total_ms() / 1000
        } else {
            let left = self.total().saturating_sub(self.done) as f64;
            let rate = self.rate();
            if rate > 0.0 { (left / rate) as u64 } else { 0 }
        };
        let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
        if complete {
            tr("task_done", true, &[self.sel.world.clone(), self.done.to_string(), percent, h.to_string(), format!("{m:02}"), format!("{s:02}")])
        } else {
            tr(
                "task_update",
                true,
                &[self.sel.world.clone(), self.done.to_string(), percent, h.to_string(), format!("{m:02}"), format!("{s:02}"), format!("{:.1}", self.rate()), self.last.0.to_string(), self.last.1.to_string()],
            )
        }
    }
}

/// A world's `region`, `entities` or `poi` folder: the 26.x layout (`<level>/dimensions/<ns>/<path>/region`)
/// when it is there, else Bukkit's (`world/region`, `world_nether/DIM-1/region`).
pub fn region_dir(world: &World, kind: &str) -> PathBuf {
    let folder = world.get_world_folder();
    let dimension = match world.get_environment() {
        "NETHER" => "the_nether",
        "THE_END" => "the_end",
        _ => "overworld",
    };
    let key = world.get_key();
    let (ns, path) = (key.get_namespace().to_owned(), key.get_key().to_owned());
    for candidate in [folder.join("dimensions").join(ns).join(path).join(kind), folder.join("dimensions").join("minecraft").join(dimension).join(kind)] {
        if candidate.is_dir() {
            return candidate;
        }
    }
    match world.get_environment() {
        "NETHER" => folder.join("DIM-1").join(kind),
        "THE_END" => folder.join("DIM1").join(kind),
        _ => folder.join(kind),
    }
}

#[derive(Clone)]
enum Pending {
    Cancel(Option<String>),
    Start(Selection),
    Trim(trim::Job),
}

struct State {
    selection: Option<Selection>,
    tasks: HashMap<String, Task>,
    pending: HashMap<String, Pending>,
    silent: bool,
    update_interval: f64,
    force_existing: bool,
    continue_on_restart: bool,
    limit: f64,
    holding: bool,
}

impl Default for State {
    fn default() -> Self {
        Self { selection: None, tasks: HashMap::new(), pending: HashMap::new(), silent: false, update_interval: 1.0, force_existing: false, continue_on_restart: false, limit: f64::MAX, holding: false }
    }
}

fn state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(&mut STATE.lock().unwrap_or_else(|e| e.into_inner()))
}

pub fn data_dir() -> PathBuf {
    plugin().map(|p| p.get_data_folder()).unwrap_or_else(|| PathBuf::from("plugins/Chunky"))
}

fn load_config() {
    let dir = data_dir();
    let c = Config::load(&dir, "config.yml", DEFAULT_CONFIG);
    let limit = std::fs::read_to_string(dir.join(".chunky.properties"))
        .ok()
        .and_then(|t| t.lines().find_map(|l| l.trim().strip_prefix("radius-limit").map(|v| v.trim_start_matches([' ', '=', ':']).trim().to_owned())))
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(f64::MAX);
    state(|s| {
        s.silent = c.bool("silent", false);
        s.update_interval = c.num("update-interval", 1.0).max(0.0);
        s.force_existing = c.bool("force-load-existing-chunks", false);
        s.continue_on_restart = c.bool("continue-on-restart", false);
        s.limit = limit;
    });
}

fn save_setting(path: &str, value: serde_yaml::Value) {
    let mut c = Config::load(&data_dir(), "config.yml", DEFAULT_CONFIG);
    c.set(path, value);
    c.save();
}

fn selection() -> Selection {
    state(|s| s.selection.get_or_insert_with(Selection::default).clone())
}

fn set_selection(f: impl FnOnce(&mut Selection)) {
    state(|s| f(s.selection.get_or_insert_with(Selection::default)));
}

// ---- tasks/<world>.properties ----

fn task_path(world: &str) -> PathBuf {
    data_dir().join("tasks").join(format!("{}.properties", world.replace(['/', '\\', ':'], "_")))
}

fn save_task(t: &Task) {
    let s = &t.sel;
    let mut text = format!("#Chunky task\nworld={}\ncancelled={}\ncenter-x={}\ncenter-z={}\nradius={}\n", s.world, t.cancelled, s.center_x, s.center_z, s.radius_x);
    if s.uses_z() {
        text.push_str(&format!("radius-z={}\n", s.radius_z));
    }
    text.push_str(&format!("pattern={}\n", s.pattern));
    if let Some(v) = &s.pattern_value {
        text.push_str(&format!("csv={v}\n"));
    }
    text.push_str(&format!("shape={}\nchunks={}\ntime={}\n", s.shape, t.done, t.total_ms()));
    let path = task_path(&s.world);
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")));
    let _ = std::fs::write(path, text);
}

fn load_saved(world: &str) -> Option<(Selection, u64, u64, bool)> {
    let text = std::fs::read_to_string(task_path(world)).ok()?;
    let props: HashMap<&str, &str> = text.lines().filter(|l| !l.starts_with('#')).filter_map(|l| l.split_once('=')).map(|(k, v)| (k.trim(), v.trim())).collect();
    let f = |k: &str, d: f64| props.get(k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(d);
    let rx = f("radius", 500.0);
    let sel = Selection {
        world: props.get("world").map_or_else(|| world.to_owned(), |w| (*w).to_owned()),
        center_x: f("center-x", 0.0),
        center_z: f("center-z", 0.0),
        radius_x: rx,
        radius_z: f("radius-z", rx),
        pattern: props.get("pattern").unwrap_or(&"region").to_string(),
        pattern_value: props.get("csv").map(|v| (*v).to_owned()),
        shape: props.get("shape").unwrap_or(&"square").to_string(),
    };
    let chunks = props.get("chunks").and_then(|v| v.parse().ok()).unwrap_or(0);
    let time = props.get("time").and_then(|v| v.parse().ok()).unwrap_or(0);
    let cancelled = props.get("cancelled").is_some_and(|v| *v == "true");
    Some((sel, chunks, time, cancelled))
}

fn start_task(sel: Selection, done: u64, prev_ms: u64) -> bool {
    let Some(task) = Task::new(sel.clone(), done, prev_ms) else { return false };
    state(|s| s.tasks.insert(sel.world.clone(), task));
    hold(true);
    true
}

/// An empty server pauses its ticks; while a task runs, pausing is held off (Chunky turns off
/// `pause-when-empty-seconds` for the same reason).
fn hold(on: bool) {
    let changed = state(|s| {
        let c = s.holding != on;
        s.holding = on;
        c
    });
    if changed && let Some(p) = plugin() {
        horizon_bukkit::fam_core::allow_pausing(&p, !on);
    }
}

/// Stops a task: tickets let go, progress saved (`cancelled` = it can't be continued, file removed).
fn stop_task(world: &str, cancelled: bool) -> bool {
    let task = state(|s| s.tasks.remove(world));
    let Some(mut t) = task else { return false };
    t.cancelled = cancelled;
    if let Some(w) = horizon_bukkit::get_world(&t.sel.world) {
        for (x, z) in t.in_flight.drain(..) {
            w.set_chunk_force_loaded(x, z, false);
        }
    }
    if cancelled {
        let _ = std::fs::remove_file(task_path(world));
    } else {
        save_task(&t);
    }
    console("task_stopped", &[world.to_owned()]);
    if state(|s| s.tasks.is_empty()) {
        hold(false);
    }
    true
}

/// The tick: each task's tickets checked and topped up.
fn tick() {
    let worlds: Vec<String> = state(|s| s.tasks.keys().cloned().collect());
    if worlds.is_empty() {
        return;
    }
    let (force, silent, interval) = state(|s| (s.force_existing, s.silent, s.update_interval));
    for name in worlds {
        let Some(world) = horizon_bukkit::get_world(&name) else {
            stop_task(&name, false);
            continue;
        };
        let Some(mut task) = state(|s| s.tasks.remove(&name)) else { continue };
        // Tickets whose chunk is made are let go.
        let mut still = Vec::with_capacity(task.in_flight.len());
        for (x, z) in std::mem::take(&mut task.in_flight) {
            if world.get_chunk_at(x, z).is_loaded() {
                world.set_chunk_force_loaded(x, z, false);
                task.counted(x, z, true);
            } else {
                still.push((x, z));
            }
        }
        task.in_flight = still;
        // New tickets.
        let mut steps = 0;
        while !task.exhausted && task.in_flight.len() < MAX_WORKING && steps < STEPS_PER_TICK {
            steps += 1;
            let Some((x, z)) = task.iter.next() else {
                task.exhausted = true;
                break;
            };
            if !task.shape.is_bounding(((x << 4) + 8) as f64, ((z << 4) + 8) as f64) || (!force && task.on_disk(x, z)) {
                task.counted(x, z, false);
                continue;
            }
            world.set_chunk_force_loaded(x, z, true);
            task.in_flight.push((x, z));
        }
        if task.exhausted && task.in_flight.is_empty() {
            horizon_bukkit::get_console_sender().send_message(&task.update_line(true));
            let _ = std::fs::remove_file(task_path(&name));
            if state(|s| s.tasks.is_empty()) {
                hold(false);
            }
            continue;
        }
        if task.last_update.is_none_or(|t| t.elapsed().as_secs_f64() > interval) {
            if !silent {
                horizon_bukkit::get_console_sender().send_message(&task.update_line(false));
            }
            task.last_update = Some(Instant::now());
        }
        state(|s| s.tasks.insert(name, task));
    }
}

// ---- Commands ----

/// `Input.tryDoubleSuffixed`: `c` chunks, `k` thousands, `r` regions; within the world's limit.
fn d(arg: &str) -> Option<f64> {
    let last = arg.chars().last()?;
    let mul = match last.to_ascii_lowercase() {
        'c' => Some(16.0),
        'k' => Some(1000.0),
        'r' => Some(512.0),
        _ => None,
    };
    match mul {
        Some(m) => arg[..arg.len() - 1].parse::<f64>().ok().map(|v| v * m),
        None => arg.parse::<f64>().ok(),
    }
    .filter(|v| v.is_finite() && v.abs() <= 30_000_000.0)
}

pub const SHAPES: [&str; 9] = ["circle", "diamond", "ellipse", "hexagon", "pentagon", "rectangle", "square", "star", "triangle"];
const PATTERNS: [&str; 6] = ["concentric", "loop", "spiral", "csv", "region", "world"];
const SUBS: [&str; 20] = [
    "cancel", "center", "confirm", "continue", "corners", "help", "pattern", "pause", "progress", "quiet", "radius", "reload", "selection", "shape", "silent", "spawn", "start", "trim", "world", "worldborder",
];

pub fn world_arg(arg: &str) -> Option<World> {
    horizon_bukkit::get_world(arg).or_else(|| horizon_bukkit::get_worlds().into_iter().find(|w| w.get_name().eq_ignore_ascii_case(arg)))
}

fn help(sender: &CommandSender, page: usize) {
    let lines: Vec<String> = [
        "help_start", "help_pause", "help_continue", "help_cancel", "help_trim", "help_world", "help_shape", "help_center", "help_radius", "help_pattern", "help_corners", "help_spawn",
        "help_worldborder", "help_selection", "help_progress", "help_silent", "help_quiet", "help_reload",
    ]
    .iter()
    .map(|k| tr(k, false, &[]))
    .collect();
    let per = 8;
    let pages = lines.len().div_ceil(per);
    let page = page.clamp(1, pages);
    let body: String = lines.iter().skip((page - 1) * per).take(per).map(|l| format!("\n{l}")).collect();
    sender.send_message(&tr("help_menu", false, &[body]));
    if page < pages {
        sender.send_message(&tr("help_more", false, &[format!("/chunky help {}", page + 1)]));
    }
}

fn start_now(sender: &CommandSender, sel: Selection) {
    if start_task(sel.clone(), 0, 0) {
        say(sender, "format_start", &[sel.world.clone(), tr(&format!("shape_{}", sel.shape), false, &[]), num(sel.center_x), num(sel.center_z), sel.radius_text()]);
    }
}

fn world_names(arg: Option<&&str>, fallback: impl FnOnce() -> Vec<String>) -> Vec<String> {
    match arg {
        Some(w) => vec![world_arg(w).map_or_else(|| (*w).to_owned(), |w| w.get_name().to_owned())],
        None => fallback(),
    }
}

fn start_command(sender: &CommandSender, a: &[&str], key: String) {
    let bad = || sender.send_message(&tr("help_start", false, &[]));
    if let Some(w) = a.first() {
        match world_arg(w) {
            Some(w) => set_selection(|s| s.world = w.get_name().to_owned()),
            None => return bad(),
        }
    }
    if let Some(sh) = a.get(1) {
        let sh = sh.to_ascii_lowercase();
        if !SHAPES.contains(&sh.as_str()) && sh != "oval" {
            return bad();
        }
        set_selection(|s| s.shape = sh);
    }
    if a.len() > 2 {
        match (a.get(2).and_then(|v| d(v)), a.get(3).and_then(|v| d(v))) {
            (Some(x), Some(z)) => set_selection(|s| (s.center_x, s.center_z) = (x, z)),
            _ => return bad(),
        }
    }
    if let Some(r) = a.get(4) {
        match d(r).filter(|r| *r >= 0.0) {
            Some(r) => set_selection(|s| (s.radius_x, s.radius_z) = (r, r)),
            None => return bad(),
        }
    }
    if let Some(r) = a.get(5) {
        match d(r).filter(|r| *r >= 0.0) {
            Some(r) => set_selection(|s| s.radius_z = r),
            None => return bad(),
        }
    }
    let sel = selection();
    let (running, limit) = state(|s| (s.tasks.contains_key(&sel.world), s.limit));
    if running {
        say(sender, "format_started_already", &[sel.world.clone()]);
    } else if sel.radius_x > limit {
        say(sender, "format_start_limit", &[num(limit)]);
    } else if load_saved(&sel.world).is_some_and(|(_, _, _, cancelled)| !cancelled) {
        state(|s| s.pending.insert(key, Pending::Start(sel)));
        say(sender, "format_start_confirm", &["/chunky continue".into(), "/chunky confirm".into()]);
    } else {
        start_now(sender, sel);
    }
}

fn confirm(sender: &CommandSender, key: &str) {
    match state(|s| s.pending.remove(key)) {
        None => say(sender, "format_confirm", &[]),
        Some(Pending::Start(sel)) => start_now(sender, sel),
        Some(Pending::Trim(job)) => trim::run(job),
        Some(Pending::Cancel(world)) => {
            let worlds: Vec<String> = match world {
                Some(w) => vec![w],
                None => {
                    say(sender, "format_cancel_all", &[]);
                    let mut all: Vec<String> = state(|s| s.tasks.keys().cloned().collect());
                    for w in horizon_bukkit::get_worlds() {
                        if task_path(w.get_name()).exists() && !all.iter().any(|n| n == w.get_name()) {
                            all.push(w.get_name().to_owned());
                        }
                    }
                    all
                }
            };
            if worlds.is_empty() {
                say(sender, "format_cancel_no_tasks", &[]);
            }
            for w in worlds {
                if !stop_task(&w, true) {
                    let _ = std::fs::remove_file(task_path(&w));
                }
                say(sender, "format_cancel", &[w]);
            }
        }
    }
}

fn command(sender: &CommandSender, args: &[&str]) -> bool {
    let sub = args.first().map(|a| a.to_ascii_lowercase()).unwrap_or_else(|| "help".into());
    if !SUBS.contains(&sub.as_str()) {
        help(sender, 1);
        return true;
    }
    if !sender.has_permission(&format!("chunky.command.{sub}")) {
        sender.send_message(&tr("command_no_permission", true, &[]));
        return true;
    }
    let a = &args[1..];
    let key = sender.get_name().to_owned();
    match sub.as_str() {
        "start" => start_command(sender, a, key),
        "confirm" => confirm(sender, &key),
        "pause" => {
            let mut any = false;
            for w in world_names(a.first(), || state(|s| s.tasks.keys().cloned().collect())) {
                if stop_task(&w, false) {
                    say(sender, "format_pause", &[w]);
                    any = true;
                }
            }
            if !any {
                say(sender, "format_pause_no_tasks", &[]);
            }
        }
        "continue" => {
            let mut any = false;
            for w in world_names(a.first(), || horizon_bukkit::get_worlds().iter().map(|w| w.get_name().to_owned()).collect()) {
                if state(|s| s.tasks.contains_key(&w)) {
                    say(sender, "format_started_already", &[w]);
                    any = true;
                    continue;
                }
                if let Some((sel, chunks, time, cancelled)) = load_saved(&w)
                    && !cancelled
                    && start_task(sel, chunks, time)
                {
                    say(sender, "format_continue", &[w]);
                    any = true;
                }
            }
            if !any {
                say(sender, "format_continue_no_tasks", &[]);
            }
        }
        "cancel" => {
            let world = a.first().map(|w| world_arg(w).map_or_else(|| (*w).to_owned(), |w| w.get_name().to_owned()));
            state(|s| s.pending.insert(key, Pending::Cancel(world)));
            say(sender, "format_cancel_confirm", &["/chunky confirm".into()]);
        }
        "progress" => {
            let lines: Vec<String> = state(|s| s.tasks.values().map(|t| t.update_line(false)).collect());
            if lines.is_empty() {
                say(sender, "format_progress_no_tasks", &[]);
            }
            for l in lines {
                sender.send_message(&l);
            }
        }
        "world" => match a.first().and_then(|w| world_arg(w)) {
            Some(w) => {
                set_selection(|s| s.world = w.get_name().to_owned());
                say(sender, "format_world", &[w.get_name().to_owned()]);
            }
            None => sender.send_message(&tr("help_world", false, &[])),
        },
        "shape" => match a.first().map(|s| s.to_ascii_lowercase()).filter(|s| SHAPES.contains(&s.as_str()) || s == "oval") {
            Some(sh) => {
                set_selection(|s| s.shape = sh.clone());
                say(sender, "format_shape", &[tr(&format!("shape_{sh}"), false, &[])]);
            }
            None => sender.send_message(&tr("help_shape", false, &[])),
        },
        "pattern" => match a.first().map(|s| s.to_ascii_lowercase()).filter(|s| PATTERNS.contains(&s.as_str())) {
            Some(p) => {
                let value = a.get(1).map(|v| (*v).to_owned());
                if p == "csv" && value.as_ref().is_none_or(|v| !data_dir().join(format!("{v}.csv")).is_file()) {
                    sender.send_message(&tr("help_pattern", false, &[]));
                    return true;
                }
                set_selection(|s| {
                    s.pattern = p.clone();
                    s.pattern_value = value;
                });
                say(sender, "format_pattern", &[tr(&format!("pattern_{p}"), false, &[])]);
            }
            None => sender.send_message(&tr("help_pattern", false, &[])),
        },
        "center" => match (a.first().and_then(|v| d(v)), a.get(1).and_then(|v| d(v))) {
            (Some(x), Some(z)) => {
                set_selection(|s| (s.center_x, s.center_z) = (x, z));
                say(sender, "format_center", &[num(x), num(z)]);
            }
            _ => match sender.as_player() {
                Some(p) if a.is_empty() => {
                    let l = p.get_location();
                    let (x, z) = (l.get_x().floor(), l.get_z().floor());
                    let world = p.get_world().get_name().to_owned();
                    set_selection(|s| {
                        (s.center_x, s.center_z) = (x, z);
                        s.world = world;
                    });
                    say(sender, "format_center", &[num(x), num(z)]);
                }
                _ => sender.send_message(&tr("help_center", false, &[])),
            },
        },
        "radius" => match a.first().and_then(|v| radius_arg(v)) {
            Some(rx) => {
                let rz = a.get(1).and_then(|v| radius_arg(v));
                set_selection(|s| {
                    s.radius_x = rx;
                    s.radius_z = rz.unwrap_or(rx);
                });
                match rz {
                    Some(rz) => say(sender, "format_radii", &[num(rx), num(rz)]),
                    None => say(sender, "format_radius", &[num(rx)]),
                }
            }
            None => sender.send_message(&tr("help_radius", false, &[])),
        },
        "corners" => match (a.first().and_then(|v| d(v)), a.get(1).and_then(|v| d(v)), a.get(2).and_then(|v| d(v)), a.get(3).and_then(|v| d(v))) {
            (Some(x1), Some(z1), Some(x2), Some(z2)) => {
                let (cx, cz) = ((x1 + x2) / 2.0, (z1 + z2) / 2.0);
                let (rx, rz) = ((x1 - x2).abs() / 2.0, (z1 - z2).abs() / 2.0);
                let square = (rx - rz).abs() < f64::EPSILON;
                set_selection(|s| {
                    (s.center_x, s.center_z, s.radius_x, s.radius_z) = (cx, cz, rx, rz);
                    s.shape = if square { "square".into() } else { "rectangle".into() };
                });
                say(sender, "format_center", &[num(cx), num(cz)]);
                if square {
                    say(sender, "format_radius", &[num(rx)]);
                } else {
                    say(sender, "format_radii", &[num(rx), num(rz)]);
                }
            }
            _ => sender.send_message(&tr("help_corners", false, &[])),
        },
        "spawn" => {
            if let Some(w) = horizon_bukkit::get_world(&selection().world) {
                let l = w.get_spawn_location();
                let (x, z) = (l.get_x().floor(), l.get_z().floor());
                set_selection(|s| (s.center_x, s.center_z) = (x, z));
                say(sender, "format_center", &[num(x), num(z)]);
            }
        }
        "worldborder" => {
            if let Some(w) = horizon_bukkit::get_world(&selection().world) {
                let b = w.get_world_border();
                let (c, r) = (b.get_center(), b.get_size() / 2.0);
                set_selection(|s| {
                    (s.center_x, s.center_z, s.radius_x, s.radius_z) = (c.get_x(), c.get_z(), r, r);
                    if s.uses_z() {
                        s.shape = "square".into();
                    }
                });
                say(sender, "format_center", &[num(c.get_x()), num(c.get_z())]);
                say(sender, "format_radius", &[num(r)]);
            }
        }
        "selection" => {
            let s = selection();
            say(sender, "format_selection", &[]);
            sender.send_message(&tr("format_selection_world", false, &[s.world.clone()]));
            sender.send_message(&tr("format_selection_shape", false, &[tr(&format!("shape_{}", s.shape), false, &[])]));
            sender.send_message(&tr("format_selection_center", false, &[num(s.center_x), num(s.center_z)]));
            if s.uses_z() {
                sender.send_message(&tr("format_selection_radii", false, &[num(s.radius_x), num(s.radius_z)]));
            } else {
                sender.send_message(&tr("format_selection_radius", false, &[num(s.radius_x)]));
            }
        }
        "silent" => {
            let on = state(|s| {
                s.silent = !s.silent;
                s.silent
            });
            save_setting("silent", serde_yaml::Value::Bool(on));
            say(sender, "format_silent", &[tr(if on { "enabled" } else { "disabled" }, false, &[])]);
        }
        "quiet" => match a.first().and_then(|v| v.parse::<i64>().ok()).filter(|v| *v >= 0) {
            Some(q) => {
                state(|s| s.update_interval = q as f64);
                save_setting("update-interval", serde_yaml::Value::Number(q.into()));
                say(sender, "format_quiet", &[q.to_string()]);
            }
            None => sender.send_message(&tr("help_quiet", false, &[])),
        },
        "reload" => {
            if state(|s| !s.tasks.is_empty()) {
                say(sender, "format_reload_tasks_running", &[]);
            } else {
                load_config();
                say(sender, "format_reload", &[]);
            }
        }
        "trim" => match trim::parse(&selection(), a) {
            Ok(job) => {
                let k = if job.inside { "format_trim_confirm_inside" } else { "format_trim_confirm" };
                let args = [job.sel.world.clone(), tr(&format!("shape_{}", job.sel.shape), false, &[]), num(job.sel.center_x), num(job.sel.center_z), job.sel.radius_text(), "/chunky confirm".into()];
                state(|s| s.pending.insert(key, Pending::Trim(job)));
                say(sender, k, &args);
            }
            Err(()) => sender.send_message(&tr("help_trim", false, &[])),
        },
        _ => help(sender, a.first().and_then(|p| p.parse().ok()).unwrap_or(1)),
    }
    true
}

fn radius_arg(v: &str) -> Option<f64> {
    // `+100` / `-100` change the radius by that much.
    let current = selection().radius_x;
    if let Some(rest) = v.strip_prefix('+') {
        return d(rest).map(|n| current + n).filter(|r| *r >= 0.0);
    }
    if let Some(rest) = v.strip_prefix('-') {
        return d(rest).map(|n| current - n).filter(|r| *r >= 0.0);
    }
    d(v).filter(|r| *r >= 0.0)
}

fn complete(args: &[&str]) -> Vec<String> {
    let typed = args.last().map(|a| a.to_ascii_lowercase()).unwrap_or_default();
    let worlds = || horizon_bukkit::get_worlds().iter().map(|w| w.get_name().to_owned()).collect::<Vec<_>>();
    let opts: Vec<String> = match (args.len(), args.first().map(|a| a.to_ascii_lowercase()).as_deref()) {
        (0 | 1, _) => SUBS.iter().map(|s| (*s).to_owned()).collect(),
        (2, Some("world" | "start" | "pause" | "continue" | "cancel" | "trim")) => worlds(),
        (2, Some("shape")) | (3, Some("start" | "trim")) => SHAPES.iter().map(|s| (*s).to_owned()).collect(),
        (2, Some("pattern")) => PATTERNS.iter().map(|s| (*s).to_owned()).collect(),
        (8, Some("trim")) => vec!["outside".into(), "inside".into()],
        _ => Vec::new(),
    };
    opts.into_iter().filter(|o| o.to_ascii_lowercase().starts_with(&typed)).collect()
}

// ---- Enable / disable ----

fn on_enable(plugin: Plugin) {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(plugin);
    let _ = std::fs::create_dir_all(data_dir().join("tasks"));
    load_config();
    plugin.set_executor("chunky", |sender, _, args| command(sender, args));
    plugin.set_tab_completer("chunky", |_, _, args| complete(args));
    plugin.run_task_timer(1, 1, tick);
    plugin.run_task_timer(20, 20, trim::report);
    if state(|s| s.continue_on_restart) {
        // After the worlds are up.
        plugin.run_task_later(20, || {
            for w in horizon_bukkit::get_worlds() {
                if let Some((sel, chunks, time, cancelled)) = load_saved(w.get_name())
                    && !cancelled
                    && start_task(sel, chunks, time)
                {
                    console("format_continue", &[w.get_name().to_owned()]);
                }
            }
        });
    }
}

fn on_disable(_plugin: Plugin) {
    let worlds: Vec<String> = state(|s| s.tasks.keys().cloned().collect());
    for w in worlds {
        stop_task(&w, false);
    }
    trim::stop();
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = State::default();
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

bukkit_plugin!(on_enable, on_disable);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_and_numbers() {
        assert_eq!(num(500.0), "500");
        assert_eq!(num(12.345), "12.35");
        assert_eq!(d("2k"), Some(2000.0));
        assert_eq!(d("10c"), Some(160.0));
        assert_eq!(d("1r"), Some(512.0));
        assert!(TRANSLATIONS.contains_key("task_update"));
        assert!(tr("format_radius", true, &["5".into()]).ends_with("Radius changed to 5."));
    }
}
