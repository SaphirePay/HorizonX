//! What HorizonX adds to the launcher's servers (`servers.rs` drives `horizon-server`):
//!
//! - the HorizonsX engine: every server runs it (`engine.path`), from `engines/horizonsx/` beside
//!   the app (`HORIZON_HOST_ENGINE` overrides);
//! - Horizon's Rust plugins, shipped in `plugins/` beside the app (`HORIZON_HOST_PLUGINS`): a catalog,
//!   installed per server by copying the library into its `plugins/` (HorizonsX hot-loads it);
//! - the public address with no port forwarding: the playit plugin goes into every server and writes
//!   `plugins/playit-gg/status.json` (its claim link, then the address), read here;
//! - schedules (a backup every N hours, a daily restart) kept in the server's `host.json`;
//! - the server's files, listed and edited as text.

use crate::backend;
use crate::servers::{self, Problem};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// The playit plugin's library name in the bundle and in a server's plugins folder.
pub const PLAYIT: &str = if cfg!(windows) { "playit_rs.dll" } else { "libplayit_rs.so" };

fn beside_app(rel: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join(rel))
}

/// The HorizonsX engine every new server runs, when it's there.
pub fn engine() -> Option<PathBuf> {
    let program = if cfg!(windows) { "horizon-client.exe" } else { "horizon-client" };
    std::env::var_os("HORIZON_HOST_ENGINE")
        .map(PathBuf::from)
        .or_else(|| beside_app(&format!("engines/horizonsx/{program}")))
        .map(|p| p.components().collect::<PathBuf>())
        .filter(|p| p.is_file())
}

/// The folder of plugins shipped with the app.
pub fn bundled_plugins() -> Option<PathBuf> {
    std::env::var_os("HORIZON_HOST_PLUGINS")
        .map(PathBuf::from)
        .or_else(|| beside_app("plugins"))
        .filter(|p| p.is_dir())
}

/// Makes a server HorizonX's: HorizonsX as its engine and the playit plugin in. Safe to run
/// again (on every server opened), it only fills in what's missing. Returns what it did.
pub fn adopt(dir: &Path) -> Vec<String> {
    let mut did = vec![];
    let plain = schedule(dir).engine == "horizon";
    if let Some(engine) = engine().filter(|_| !plain) {
        let config = servers::config(dir);
        let at = |key: &str| config.as_ref().and_then(|c| c.pointer(key).and_then(Value::as_str).map(str::to_string));
        let mut changes = vec![];
        if at("/engine/path").as_deref().map(Path::new) != Some(engine.as_path()) {
            changes.push(("engine.path", engine.display().to_string()));
        }
        // An engine outside the runtime looks for the game files beside itself: point it at the
        // prepared ones the servers share.
        let resources = servers::game_files(&servers::runtime_root(), "26.3");
        if at("/engine/resources").unwrap_or_default().is_empty() && resources.is_dir() {
            changes.push(("engine.resources", resources.display().to_string()));
        }
        if !changes.is_empty() {
            match servers::set_config(dir, &changes) {
                Ok(_) => did.push("Engine: HorizonsX".to_string()),
                Err(e) => did.push(format!("Couldn't set the HorizonsX engine: {}", e.message)),
            }
        }
    }
    // playit (a public address) is never put in here: only the owner turns it on (`set_playit`), so a server
    // HorizonX takes over stays on its own network until they choose otherwise.
    if let Some(note) = make_private_once(dir) {
        did.push(note);
    }
    // The app shipped newer builds of plugins this server has (after an update): put them in.
    let fresh = refresh_plugins(dir);
    if fresh > 0 {
        did.push(format!("Plugins: {fresh} updated"));
    }
    if has_jars(dir) {
        match ensure_java_layer(dir) {
            Ok(true) => did.push("Java plugins: the Java layer is in".to_string()),
            Ok(false) => {}
            Err(e) => did.push(format!("Couldn't add the Java layer: {e}")),
        }
    }
    did
}

// ----- Java plugins ------------------------------------------------------------------------------

/// The Java layer shipped with the app (`java/` beside it; `HORIZON_HOST_JAVA` overrides): the
/// server plugin `horizon_java.dll`, `lib/` (the layer's jar, the Paper API and its libraries) and
/// `runtime/` (a Java 25 runtime), so jar plugins run with nothing else installed.
pub fn bundled_java() -> Option<PathBuf> {
    std::env::var_os("HORIZON_HOST_JAVA")
        .map(PathBuf::from)
        .or_else(|| beside_app("java"))
        .filter(|p| p.join("lib").join("horizon-javalayer.jar").is_file())
}

fn java_layer_file() -> &'static str {
    if cfg!(windows) { "horizon_java.dll" } else { "libhorizon_java.so" }
}

fn has_jars(dir: &Path) -> bool {
    fs::read_dir(dir.join("plugins"))
        .map(|r| r.flatten().any(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("jar"))))
        .unwrap_or(false)
}

/// Links `link` to the folder `target`: a junction on Windows (no admin rights needed), a symlink
/// elsewhere. A link that points nowhere (the app was moved) is replaced; a real folder is kept.
fn link_folder(link: &Path, target: &Path) -> Result<(), String> {
    if link.exists() {
        return Ok(());
    }
    // A dangling link: `exists()` follows it and says no, but the link itself is there.
    if fs::symlink_metadata(link).is_ok() {
        fs::remove_dir(link).or_else(|_| fs::remove_file(link)).map_err(|e| e.to_string())?;
    }
    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    #[cfg(windows)]
    {
        let status = crate::process::command("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("couldn't link {}", link.display()));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(target, link).map_err(|e| e.to_string())
    }
}

/// Puts the bundled Java layer into a server: the server plugin into `plugins/` (refreshed when the
/// app ships a newer one and the server isn't holding it), and `java-layer/lib` and
/// `java-layer/runtime` linked to the app's copies. Returns whether the plugin was newly added.
pub fn ensure_java_layer(dir: &Path) -> Result<bool, String> {
    if schedule(dir).java_layer_off {
        return Ok(false);
    }
    let Some(java) = bundled_java() else { return Ok(false) };
    let file = java_layer_file();
    let source = java.join(file);
    let target = dir.join("plugins").join(file);
    let added = !target.is_file();
    let newer = |a: &Path, b: &Path| {
        let time = |p: &Path| fs::metadata(p).and_then(|m| m.modified()).ok();
        matches!((time(a), time(b)), (Some(x), Some(y)) if x > y)
    };
    if source.is_file() && (added || newer(&source, &target)) {
        fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
        // A running server holds the library: the next restart picks up the new one.
        if let Err(e) = fs::copy(&source, &target) {
            if added {
                return Err(e.to_string());
            }
        }
    }
    let layer = dir.join("java-layer");
    link_folder(&layer.join("lib"), &java.join("lib"))?;
    if java.join("runtime").is_dir() {
        link_folder(&layer.join("runtime"), &java.join("runtime"))?;
    }
    Ok(added)
}

/// Whether the server runs HorizonsX.
pub fn runs_horizonsx(dir: &Path) -> bool {
    let Some(engine) = engine() else { return false };
    servers::config(dir)
        .and_then(|c| c.pointer("/engine/path").and_then(Value::as_str).map(PathBuf::from))
        .is_some_and(|p| p == engine)
}

// ----- Getting ready (first run) ------------------------------------------------------------------

/// Whether servers can be made now: the server programs and the prepared game files are in place.
pub fn ready() -> bool {
    servers::program().is_some() && servers::game_files_ready()
}

/// Everything a first server needs, done once: a Horizon version installed (the newest, when none
/// is), prepared from the player's own Minecraft 26.3, and its programs and game files set up for
/// servers. Says what it does through `log`. Returns the version used.
pub fn get_ready(log: &dyn Fn(String)) -> Result<String, String> {
    let installed = backend::installed();
    if ready() {
        return Ok(servers::runtime_release().or_else(|| installed.last().cloned()).unwrap_or_default());
    }
    // An installed version that is prepared and can host: only its files to copy.
    if let Some(tag) = installed.iter().rev().find(|t| servers::can_host(t).is_ok()) {
        log(format!("Setting up Horizon {tag} for servers"));
        servers::set_up(tag)?;
        return Ok(tag.clone());
    }
    let program = if cfg!(windows) { "horizon-server.exe" } else { "horizon-server" };
    let tag = match installed.iter().rev().find(|t| backend::version_dir(t).join(program).is_file()) {
        Some(t) => t.clone(),
        None => {
            log("Downloading the newest Horizon".into());
            let releases = backend::releases()?;
            let latest = backend::latest(&releases).ok_or("No Horizon release was found")?.clone();
            backend::install(&latest, |p| {
                let _ = p;
            })?;
            latest.tag
        }
    };
    if !backend::check_prepared(&tag) {
        log("Preparing Minecraft 26.3 from your own copy (once)".into());
        let minecraft = backend::minecraft_dir(None)?;
        let (preparer, minecraft) = backend::preparer(&tag, &minecraft, backend::MinecraftFrom::Install, log)?;
        backend::sounds(&tag, &minecraft, log);
        match preparer {
            backend::Preparer::Process(mut cmd) => {
                // Its output goes to `log`, never to this process's stdout: under `HorizonX mcp` stdout is the
                // protocol, and a line of the preparer there breaks the AI assistant's connection.
                let out = cmd
                    .stdin(std::process::Stdio::null())
                    .output()
                    .map_err(|e| format!("Preparing Minecraft 26.3: couldn't run the preparer: {e}"))?;
                let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                for l in text.lines().filter(|l| !l.trim().is_empty()) {
                    log(l.to_string());
                }
                if !out.status.success() {
                    let tail: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
                    let tail = tail[tail.len().saturating_sub(6)..].join("
");
                    return Err(format!("Preparing Minecraft 26.3 failed ({}): {}", out.status, if tail.is_empty() { "the preparer said nothing" } else { &tail }));
                }
            }
            backend::Preparer::InProcess(options) => backend::prepare_in_process(&options, log)?,
        }
    }
    log(format!("Setting up Horizon {tag} for servers"));
    servers::set_up(&tag)?;
    Ok(tag)
}

// ----- Plugins ------------------------------------------------------------------------------------

/// A plugin the app ships.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plugin {
    /// The library's file name (`hsmp_rs.dll`).
    pub file: String,
    pub name: String,
    pub description: String,
    /// Put in every server (playit).
    #[serde(default)]
    pub built_in: bool,
}

/// The catalog: `catalog.json` in the plugins folder names and describes them; a library it
/// doesn't list shows under its file name.
pub fn catalog() -> Vec<Plugin> {
    let Some(folder) = bundled_plugins() else { return vec![] };
    let listed: Vec<Plugin> = fs::read_to_string(folder.join("catalog.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let mut out: Vec<Plugin> = listed.into_iter().filter(|p| folder.join(&p.file).is_file()).collect();
    let ext = if cfg!(windows) { "dll" } else { "so" };
    if let Ok(read) = fs::read_dir(&folder) {
        let mut extra: Vec<Plugin> = read
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|f| f.ends_with(&format!(".{ext}")) && !out.iter().any(|p| &p.file == f))
            .map(|f| Plugin { name: f.trim_end_matches(&format!(".{ext}")).to_string(), file: f, description: String::new(), built_in: false })
            .collect();
        extra.sort_by(|a, b| a.name.cmp(&b.name));
        out.extend(extra);
    }
    out
}

pub fn plugin_installed(dir: &Path, file: &str) -> bool {
    dir.join("plugins").join(file).is_file()
}

/// Replaces the server's copies of shipped plugins (on or off) with the app's when they differ, so an update of
/// the app reaches every server. Returns how many changed. One a running server holds stays for the next try.
pub fn refresh_plugins(dir: &Path) -> usize {
    let Some(bundle) = bundled_plugins() else { return 0 };
    let folder = dir.join("plugins");
    let mut n = 0;
    for (file, on) in plugin_files(dir) {
        let shipped = bundle.join(&file);
        if !shipped.is_file() {
            continue;
        }
        let here = if on { folder.join(&file) } else { folder.join(format!("{file}.disabled")) };
        if crate::app_update::same(&shipped, &here) {
            continue;
        }
        let tmp = folder.join(format!(".{file}.copying"));
        if fs::copy(&shipped, &tmp).is_ok() && fs::rename(&tmp, &here).is_ok() {
            n += 1;
        } else {
            let _ = fs::remove_file(&tmp);
        }
    }
    n
}

/// Copies a shipped plugin into the server (HorizonsX loads it at once when it runs).
pub fn install_plugin(dir: &Path, file: &str) -> Result<(), String> {
    let from = bundled_plugins().map(|f| f.join(file)).filter(|p| p.is_file()).ok_or_else(|| format!("{file} isn't in HorizonX's plugins"))?;
    let to_dir = dir.join("plugins");
    fs::create_dir_all(&to_dir).map_err(|e| e.to_string())?;
    let tmp = to_dir.join(format!(".{file}.copying"));
    fs::copy(&from, &tmp).map_err(|e| e.to_string())?;
    fs::rename(&tmp, to_dir.join(file)).map_err(|e| e.to_string())
}

/// Takes a plugin out of the server (its data folder stays).
pub fn remove_plugin(dir: &Path, file: &str) -> Result<(), String> {
    let path = dir.join("plugins").join(file);
    if path.is_file() {
        fs::remove_file(&path).map_err(|e| format!("{e} (stop the server first if it's running)"))?;
    }
    Ok(())
}

/// Every plugin file in the server (HorizonsX libraries and Java jars), on or set aside (`NAME.disabled`, as
/// HorizonsX's own `/plugin disable` leaves them): (file name without `.disabled`, on).
pub fn plugin_files(dir: &Path) -> Vec<(String, bool)> {
    let ext = if cfg!(windows) { ".dll" } else { ".so" };
    let mut out: Vec<(String, bool)> = fs::read_dir(dir.join("plugins"))
        .map(|r| {
            r.flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .filter_map(|f| match f.strip_suffix(".disabled") {
                    Some(base) => Some((base.to_string(), false)),
                    None => Some((f, true)),
                })
                .filter(|(f, _)| f.ends_with(ext) || f.to_ascii_lowercase().ends_with(".jar"))
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|(f, _)| f.to_lowercase());
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

/// Turns a plugin on or off by its file (`X.dll` <-> `X.dll.disabled`). A running HorizonsX server notices at
/// its next restart; Java plugins load at start anyway.
pub fn set_plugin_on(dir: &Path, file: &str, on: bool) -> Result<(), String> {
    let folder = dir.join("plugins");
    let (from, to) = if on { (folder.join(format!("{file}.disabled")), folder.join(file)) } else { (folder.join(file), folder.join(format!("{file}.disabled"))) };
    if !from.is_file() {
        return Ok(());
    }
    fs::rename(&from, &to).map_err(|e| format!("{e} (stop the server first if it's running)"))
}

/// Copies Java plugin jars into the server's plugins folder (they load at its next start).
pub fn add_jars(dir: &Path, jars: &[PathBuf]) -> Result<usize, String> {
    let folder = dir.join("plugins");
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let mut added = 0;
    for jar in jars.iter().filter(|j| j.extension().is_some_and(|e| e.eq_ignore_ascii_case("jar"))) {
        let name = jar.file_name().ok_or("no file name")?;
        fs::copy(jar, folder.join(name)).map_err(|e| e.to_string())?;
        added += 1;
    }
    if added > 0 {
        ensure_java_layer(dir)?;
    }
    Ok(added)
}

/// The libraries in the server's plugins folder.
pub fn installed_plugins(dir: &Path) -> Vec<String> {
    let ext = if cfg!(windows) { ".dll" } else { ".so" };
    let mut out: Vec<String> = fs::read_dir(dir.join("plugins"))
        .map(|r| r.flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|f| f.ends_with(ext)).collect())
        .unwrap_or_default();
    out.sort();
    out
}

// ----- The public address (playit) ----------------------------------------------------------------

/// What the playit plugin says (its `status.json`).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Playit {
    pub state: i32,
    #[serde(default)]
    pub state_text: String,
    pub claim_url: Option<String>,
    pub address: Option<String>,
    pub guest: Option<bool>,
    pub email_verified: Option<bool>,
    pub notice: Option<String>,
}

impl Playit {
    pub fn online(&self) -> bool {
        self.state == 12 && self.address.is_some()
    }
    pub fn waiting_for_claim(&self) -> bool {
        self.state == 2 && self.claim_url.is_some()
    }
}

pub fn playit(dir: &Path) -> Option<Playit> {
    let text = fs::read_to_string(dir.join("plugins/playit-gg/status.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// A console command to the running server (`/playit agent restart`...): what it answered.
pub fn command(dir: &Path, line: &str) -> Result<Vec<String>, Problem> {
    let api = servers::Api::open(dir).ok_or_else(|| Problem::plain("The server isn't running".to_string()))?;
    api.command(line).map_err(Problem::plain)
}

// ----- Schedules ----------------------------------------------------------------------------------

/// The server's schedules, kept in `host.json` in its folder.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    /// A backup every this many hours while it runs (0 = off).
    #[serde(default)]
    pub backup_hours: u32,
    /// A restart every day at this hour, local time (None = off).
    #[serde(default)]
    pub restart_hour: Option<u32>,
    /// A restart every this many hours the server has been up (0 = off): 6 h keeps a busy server fresh. Counted
    /// from its start, so one started a minute ago isn't restarted at once. Used instead of `restart_hour`.
    #[serde(default)]
    pub restart_every: u32,
    /// When the last scheduled backup / restart ran (Unix seconds).
    #[serde(default)]
    pub last_backup: u64,
    #[serde(default)]
    pub last_restart: u64,
    /// "horizon" when the owner chose the plain Horizon engine (not switched to HorizonsX).
    #[serde(default)]
    pub engine: String,
    /// The owner turned playit off: the app leaves it out (no public address).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub playit_off: bool,
    /// The server was made private for its public address (`make_private_once`): done once, so an owner who
    /// later turns the whitelist off on purpose isn't overruled.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub made_private: bool,
    /// The owner turned the Java layer off: jars stay, and nothing puts the layer back.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub java_layer_off: bool,
}

pub fn schedule(dir: &Path) -> Schedule {
    fs::read_to_string(dir.join("host.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save_schedule(dir: &Path, s: &Schedule) -> Result<(), String> {
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    let tmp = dir.join("host.json.tmp");
    fs::write(&tmp, text).map_err(|e| e.to_string())?;
    fs::rename(&tmp, dir.join("host.json")).map_err(|e| e.to_string())
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// What's due now for a running server: `(backup, restart)`. `local_hour` is the hour of day now, `uptime` how
/// long the server has been up (seconds).
pub fn due(s: &Schedule, local_hour: u32, uptime: Option<u64>) -> (bool, bool) {
    let t = now();
    let backup = s.backup_hours > 0 && t.saturating_sub(s.last_backup) >= s.backup_hours as u64 * 3600;
    let restart = if s.restart_every > 0 {
        let every = s.restart_every as u64 * 3600;
        uptime.is_some_and(|up| up >= every) && t.saturating_sub(s.last_restart) >= every
    } else {
        s.restart_hour == Some(local_hour) && t.saturating_sub(s.last_restart) >= 23 * 3600
    };
    (backup, restart)
}

/// Whether a scheduled restart comes in the next five minutes (`minute` is the minute of the hour now), so
/// players can be told once: true only during the one minute that is five minutes before it.
pub fn restart_soon(s: &Schedule, local_hour: u32, minute: u32, uptime: Option<u64>) -> bool {
    if s.restart_every > 0 {
        let every = s.restart_every as u64 * 3600;
        return uptime.is_some_and(|up| up + 300 >= every && up + 240 < every);
    }
    s.restart_hour.is_some_and(|h| (h + 23) % 24 == local_hour && minute == 55)
}

/// Runs what's due for a running server and records it. Returns what it did, for the page's log.
pub fn run_schedule(dir: &Path, local_hour: u32, uptime: Option<u64>) -> Vec<String> {
    let mut s = schedule(dir);
    let (backup, restart) = due(&s, local_hour, uptime);
    let mut did = vec![];
    if backup {
        s.last_backup = now();
        match servers::run(dir, &["backup"]) {
            Ok(_) => did.push("Scheduled backup made".to_string()),
            Err(e) => did.push(format!("Scheduled backup failed: {}", e.message)),
        }
    }
    if restart {
        s.last_restart = now();
        match servers::run(dir, &["restart"]) {
            Ok(_) => did.push("Scheduled restart".to_string()),
            Err(e) => did.push(format!("Scheduled restart failed: {}", e.message)),
        }
    }
    if backup || restart {
        let _ = save_schedule(dir, &s);
    }
    did
}

// ----- Files ----------------------------------------------------------------------------------------

/// One entry of a folder in the server.
#[derive(Clone, Debug, PartialEq)]
pub struct FileEntry {
    pub name: String,
    pub dir: bool,
    pub bytes: u64,
}

/// A folder inside the server, folders first. `rel` is relative to the server and may not leave it.
pub fn list_files(dir: &Path, rel: &str) -> Result<Vec<FileEntry>, String> {
    let path = inside(dir, rel)?;
    let mut out: Vec<FileEntry> = fs::read_dir(&path)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some(FileEntry { name: e.file_name().to_string_lossy().into_owned(), dir: meta.is_dir(), bytes: meta.len() })
        })
        .collect();
    out.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

/// Files that open as text in the editor.
pub fn editable(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".yml", ".yaml", ".json", ".toml", ".properties", ".txt", ".conf", ".cfg", ".log", ".md", ".csv"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// A text file's contents (at most 1 MB).
pub fn read_text(dir: &Path, rel: &str) -> Result<String, String> {
    let path = inside(dir, rel)?;
    let meta = fs::metadata(&path).map_err(|e| e.to_string())?;
    if meta.len() > 1 << 20 {
        return Err("Over 1 MB: open it in the folder instead".into());
    }
    fs::read_to_string(&path).map_err(|e| e.to_string())
}

/// Writes a text file (beside, then renamed in). JSON is checked before it's written.
pub fn write_text(dir: &Path, rel: &str, text: &str) -> Result<(), String> {
    let path = inside(dir, rel)?;
    if rel.to_ascii_lowercase().ends_with(".json") {
        serde_json::from_str::<Value>(text).map_err(|e| format!("Not valid JSON: {e}"))?;
    }
    let tmp = path.with_extension("host-edit.tmp");
    fs::write(&tmp, text).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// `rel` under `dir`, refusing `..` and absolute paths.
fn inside(dir: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel = Path::new(rel);
    if rel.is_absolute() || rel.components().any(|c| matches!(c, std::path::Component::ParentDir | std::path::Component::Prefix(_) | std::path::Component::RootDir)) {
        return Err("That path is outside the server".into());
    }
    Ok(dir.join(rel))
}

// ----- Java plugins ---------------------------------------------------------------------------------

/// A Bukkit/Spigot/Paper jar in the server's plugins folder (run by HorizonsX's Java layer), with what its
/// `plugin.yml` (or `paper-plugin.yml`) says and the settings files in its data folder.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JavaPlugin {
    /// The jar's file name.
    pub file: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub authors: Vec<String>,
    /// Its data folder, relative to the server (`plugins/AuraSkills`), when there is one.
    pub folder: Option<String>,
    /// Its settings files, relative to the server, config.yml first.
    pub configs: Vec<String>,
    pub bytes: u64,
    /// Not set aside (`NAME.jar.disabled`).
    pub on: bool,
}

/// The Java layer (horizon_java) is in the server: without it HorizonsX doesn't run jars.
pub fn java_layer_on(dir: &Path) -> bool {
    let file = if cfg!(windows) { "horizon_java.dll" } else { "libhorizon_java.so" };
    dir.join("plugins").join(file).is_file()
}

/// Turns the Java layer on or off for a server, and remembers it. Off keeps the jars and the layer as `.disabled`;
/// a running server notices at its next restart.
pub fn set_java_layer(dir: &Path, on: bool) -> Result<(), String> {
    let mut s = schedule(dir);
    s.java_layer_off = !on;
    save_schedule(dir, &s)?;
    let file = java_layer_file();
    if on {
        let off = dir.join("plugins").join(format!("{file}.disabled"));
        if off.is_file() && !dir.join("plugins").join(file).is_file() {
            set_plugin_on(dir, file, true)?;
        }
        let _ = fs::remove_file(&off);
        if bundled_java().is_some() {
            ensure_java_layer(dir)?;
        } else if !java_layer_on(dir) {
            return Err("The Java layer isn't beside the app: reinstall HorizonX.".into());
        }
        Ok(())
    } else {
        let _ = fs::remove_file(dir.join("plugins").join(format!("{file}.disabled")));
        set_plugin_on(dir, file, false)
    }
}

/// Whether playit (the public address) is on for a server.
pub fn playit_on(dir: &Path) -> bool {
    plugin_installed(dir, PLAYIT)
}

/// Turns playit on or off for a server, and remembers it. A running HorizonsX server loads or unloads it at once.
/// On, the server is private as well (`playit_privacy`): a public address is found by the scanners that sweep
/// playit.gg's shared addresses for Minecraft servers.
pub fn set_playit(dir: &Path, on: bool) -> Result<(), String> {
    switch_playit(dir, on)?;
    playit_privacy(dir, on)?;
    if on {
        let mut s = schedule(dir);
        s.made_private = true;
        save_schedule(dir, &s)?;
    }
    Ok(())
}

/// What a public address takes with it. playit.gg puts every tunnel on its shared addresses, and server
/// scanners sweep those for Minecraft servers and publish what answers: so with playit on, only the
/// whitelist joins (the owner's own account added and made an operator, so they aren't locked out), the
/// server doesn't answer status pings (a scanner gets nothing back and never lists it; a player who has
/// the address still joins) and its player names aren't shown. Off, status pings are answered again (the
/// whitelist stays as the owner left it).
pub fn playit_privacy(dir: &Path, on: bool) -> Result<(), String> {
    let changes: Vec<(&str, String)> = if on {
        vec![
            ("server.whitelist", "true".into()),
            ("server.enforce_whitelist", "true".into()),
            ("properties.enable-status", "false".into()),
            ("properties.hide-online-players", "true".into()),
        ]
    } else {
        vec![("properties.enable-status", "true".into())]
    };
    servers::set_config(dir, &changes).map_err(|e| format!("Couldn't make the server private: {}", e.message))?;
    if on {
        let_owner_in(dir);
    }
    Ok(())
}

/// The signed-in account (HorizonX shares the launcher's accounts) on the whitelist and an operator: through
/// the console when the server runs, else into whitelist.json and ops.json as vanilla writes them. Its name,
/// or None with nobody signed in.
pub fn let_owner_in(dir: &Path) -> Option<String> {
    let account = crate::account::Accounts::load().selected().cloned()?;
    let name = account.name.clone();
    let has = |file: &str| servers::names(dir, file).iter().any(|n| n.eq_ignore_ascii_case(&name));
    let (listed, opped) = (has("whitelist.json"), has("ops.json"));
    if listed && opped {
        return Some(name);
    }
    if servers::run(dir, &["cmd", "whitelist", "add", &name]).is_ok() {
        if !opped {
            let _ = servers::run(dir, &["cmd", "op", &name]);
        }
        return Some(name);
    }
    let uuid = dashed_uuid(&account.uuid);
    if !listed {
        add_list_entry(dir, "whitelist.json", serde_json::json!({ "uuid": uuid, "name": name }));
    }
    if !opped {
        add_list_entry(dir, "ops.json", serde_json::json!({ "uuid": uuid, "name": name, "level": 4, "bypassesPlayerLimit": false }));
    }
    Some(name)
}

/// A profile UUID with its dashes (8-4-4-4-12), as the list files keep it.
fn dashed_uuid(uuid: &str) -> String {
    let u: String = uuid.chars().filter(|c| *c != '-').collect();
    if u.len() != 32 {
        return uuid.to_string();
    }
    format!("{}-{}-{}-{}-{}", &u[0..8], &u[8..12], &u[12..16], &u[16..20], &u[20..32])
}

/// One more entry in a list file (`whitelist.json`, `ops.json`); a missing or unreadable file starts empty.
fn add_list_entry(dir: &Path, file: &str, entry: Value) {
    let path = dir.join(file);
    let mut list: Vec<Value> = fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    list.push(entry);
    if let Ok(text) = serde_json::to_string_pretty(&list) {
        let _ = fs::write(&path, text + "\n");
    }
}

/// HorizonX 0.3.5: a server that already had a public address (playit on, from before it was off by default)
/// is made private once, as `set_playit` makes one now: only its whitelist joins (the owner added) and server
/// scanners get no answer. What it tells the owner, or None when there was nothing to do. Run when a server is
/// opened (`adopt`) and for every server when HorizonX starts, so one that runs unattended is covered too.
pub fn make_private_once(dir: &Path) -> Option<String> {
    let mut s = schedule(dir);
    if s.made_private || !playit_on(dir) {
        return None;
    }
    let name = dir.file_name().map_or_else(|| "This server".to_string(), |n| n.to_string_lossy().into_owned());
    match playit_privacy(dir, true) {
        Ok(()) => {
            s.made_private = true;
            let _ = save_schedule(dir, &s);
            Some(format!(
                "Security update: {name} has a public playit.gg address, so it's private now. Only players on its                  whitelist can join, and server scanners can't see it. Add your friends on the Players page."
            ))
        }
        Err(e) => Some(format!("Security update: couldn't make {name} private yet ({e}); it is tried again next time.")),
    }
}

/// [`set_playit`]'s plugin switch and the remembered choice.
fn switch_playit(dir: &Path, on: bool) -> Result<(), String> {
    let mut s = schedule(dir);
    s.playit_off = !on;
    save_schedule(dir, &s)?;
    let off = dir.join("plugins").join(format!("{PLAYIT}.disabled"));
    if on {
        if plugin_installed(dir, PLAYIT) {
            let _ = fs::remove_file(&off);
            Ok(())
        } else if off.is_file() {
            set_plugin_on(dir, PLAYIT, true)
        } else {
            install_plugin(dir, PLAYIT)
        }
    } else {
        if plugin_installed(dir, PLAYIT) {
            let _ = fs::remove_file(&off);
        }
        set_plugin_on(dir, PLAYIT, false)
    }
}

/// The jars in the server's plugins folder, by name.
pub fn java_plugins(dir: &Path) -> Vec<JavaPlugin> {
    let plugins = dir.join("plugins");
    let Ok(read) = fs::read_dir(&plugins) else { return vec![] };
    let folders: Vec<String> = fs::read_dir(&plugins)
        .map(|r| r.flatten().filter(|e| e.path().is_dir()).filter_map(|e| e.file_name().into_string().ok()).collect())
        .unwrap_or_default();
    let mut out: Vec<JavaPlugin> = read
        .flatten()
        .filter_map(|e| {
            let raw = e.file_name().into_string().ok()?;
            let (file, on) = match raw.strip_suffix(".disabled") {
                Some(base) => (base.to_string(), false),
                None => (raw.clone(), true),
            };
            if !file.to_ascii_lowercase().ends_with(".jar") || !e.path().is_file() {
                return None;
            }
            let bytes = e.metadata().map(|m| m.len()).unwrap_or(0);
            let mut p = describe_jar(&e.path()).unwrap_or_default();
            if p.name.is_empty() {
                p.name = file.trim_end_matches(".jar").to_string();
            }
            p.file = file;
            p.bytes = bytes;
            p.on = on;
            // The data folder is the plugin's name; some differ in case only.
            if let Some(folder) = folders.iter().find(|f| **f == p.name).or_else(|| folders.iter().find(|f| f.eq_ignore_ascii_case(&p.name))) {
                p.folder = Some(format!("plugins/{folder}"));
                p.configs = settings_files(dir, &format!("plugins/{folder}"));
            }
            Some(p)
        })
        .collect();
    out.sort_by_key(|p| p.name.to_lowercase());
    out
}

/// Name, version, description and authors from a jar's plugin.yml or paper-plugin.yml.
fn describe_jar(jar: &Path) -> Option<JavaPlugin> {
    let mut zip = zip::ZipArchive::new(fs::File::open(jar).ok()?).ok()?;
    let text = ["paper-plugin.yml", "plugin.yml"].iter().find_map(|name| {
        let mut entry = zip.by_name(name).ok()?;
        let mut s = String::new();
        std::io::Read::read_to_string(&mut entry, &mut s).ok()?;
        Some(s)
    })?;
    let mut p = JavaPlugin::default();
    let mut in_authors = false;
    for line in text.lines() {
        // Top-level keys only (no indent); a `authors:` list continues on `- name` lines.
        if in_authors {
            if let Some(item) = line.trim_start().strip_prefix("- ").filter(|_| line.starts_with([' ', '-'])) {
                p.authors.push(yaml_value(item));
                continue;
            }
            in_authors = false;
        }
        if line.starts_with([' ', '\t', '#']) {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        match key.trim() {
            "name" => p.name = yaml_value(value),
            "version" => p.version = yaml_value(value),
            "description" => p.description = yaml_value(value),
            "author" if !value.is_empty() => p.authors.push(yaml_value(value)),
            "authors" if value.starts_with('[') => {
                p.authors.extend(value.trim_matches(['[', ']']).split(',').map(yaml_value).filter(|a| !a.is_empty()))
            }
            "authors" => in_authors = true,
            _ => {}
        }
    }
    Some(p)
}

/// A one-line YAML scalar without its quotes. Build tools leave `${project.version}` in some; shown as is.
fn yaml_value(v: &str) -> String {
    let v = v.trim();
    let v = v.split(" #").next().unwrap_or(v).trim();
    v.trim_matches(['"', '\'']).to_string()
}

/// The settings files in a plugin's data folder and one level of folders under it (`lang/`, `menus/`...), config.yml
/// first, at most 40.
fn settings_files(dir: &Path, rel: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut walk = vec![(rel.to_string(), 0)];
    while let Some((folder, depth)) = walk.pop() {
        let Ok(read) = fs::read_dir(dir.join(&folder)) else { continue };
        let mut entries: Vec<_> = read.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let Ok(name) = e.file_name().into_string() else { continue };
            let child = format!("{folder}/{name}");
            if e.path().is_dir() {
                if depth == 0 {
                    walk.push((child, 1));
                }
            } else if editable(&name) && !name.ends_with(".log") {
                out.push(child);
            }
        }
    }
    out.sort_by_key(|f| (!f.ends_with("/config.yml"), f.matches('/').count(), f.to_lowercase()));
    out.truncate(40);
    out
}

// ----- Performance history --------------------------------------------------------------------------

/// The last samples of a server's numbers, one a poll, for the graphs.
#[derive(Clone, Debug, Default)]
pub struct History {
    pub tps: Vec<f32>,
    pub mspt: Vec<f32>,
    pub ram_gb: Vec<f32>,
    pub cpu: Vec<f32>,
    pub players: Vec<f32>,
}

/// How many samples a graph keeps (one a second: five minutes).
pub const HISTORY: usize = 300;

impl History {
    pub fn push(&mut self, s: &servers::Status) {
        fn add(v: &mut Vec<f32>, x: f32) {
            v.push(x);
            if v.len() > HISTORY {
                v.remove(0);
            }
        }
        if s.phase != "running" {
            return;
        }
        add(&mut self.tps, s.tps.unwrap_or(0.0) as f32);
        add(&mut self.mspt, s.mspt.unwrap_or(0.0) as f32);
        add(&mut self.ram_gb, s.ram_bytes.unwrap_or(0) as f32 / 1e9);
        add(&mut self.cpu, s.cpu_percent.unwrap_or(0.0) as f32);
        add(&mut self.players, s.players.len() as f32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_inside() {
        let d = Path::new("C:/srv");
        assert!(inside(d, "plugins/x.yml").is_ok());
        assert!(inside(d, "../x").is_err());
        // Each system's own absolute paths (on Linux "C:/Windows/x" is a plain relative name).
        #[cfg(windows)]
        assert!(inside(d, "C:/Windows/x").is_err());
        assert!(inside(d, "/etc/passwd").is_err());
        assert!(inside(d, "plugins/../../x").is_err());
        assert!(editable("server.properties") && editable("a.YML") && !editable("world.dat"));
    }

    #[test]
    fn schedule_due() {
        let s = Schedule { backup_hours: 2, restart_hour: Some(4), restart_every: 0, last_backup: 0, last_restart: 0, engine: String::new(), ..Default::default() };
        assert_eq!(due(&s, 4, None), (true, true));
        assert_eq!(due(&s, 5, None), (true, false));
        let s = Schedule { backup_hours: 0, restart_hour: None, ..s };
        assert_eq!(due(&s, 4, None), (false, false));
    }

    #[test]
    fn restart_every_counts_from_the_start() {
        let s = Schedule { restart_every: 6, ..Schedule::default() };
        // Up 5 h 59 min: not yet. Up 6 h: due. Unknown uptime: never.
        assert_eq!(due(&s, 4, Some(6 * 3600 - 60)), (false, false));
        assert_eq!(due(&s, 4, Some(6 * 3600)), (false, true));
        assert_eq!(due(&s, 4, None), (false, false));
        // Just restarted by the schedule: not again until 6 h later, whatever the uptime says.
        let s = Schedule { last_restart: now(), ..s };
        assert_eq!(due(&s, 4, Some(7 * 3600)), (false, false));
        // A daily hour is ignored while an interval is set.
        let s = Schedule { restart_every: 6, restart_hour: Some(4), ..Schedule::default() };
        assert_eq!(due(&s, 4, Some(60)), (false, false));
    }

    #[test]
    fn playit_status_reads() {
        let p: Playit = serde_json::from_str(r#"{"state":12,"state_text":"connected","claim_url":null,"address":"a.joinmc.link","guest":false,"email_verified":true,"notice":null,"version":"0.2.0"}"#).unwrap();
        assert!(p.online() && !p.waiting_for_claim());
    }

    #[test]
    fn the_owner_goes_on_the_lists_once_with_a_dashed_uuid() {
        assert_eq!(dashed_uuid("0123456789abcdef0123456789abcdef"), "01234567-89ab-cdef-0123-456789abcdef");
        assert_eq!(dashed_uuid("01234567-89ab-cdef-0123-456789abcdef"), "01234567-89ab-cdef-0123-456789abcdef");
        let dir = std::env::temp_dir().join(format!("hh-lists-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("whitelist.json"), r#"[{"uuid":"a","name":"Friend"}]"#).unwrap();
        add_list_entry(&dir, "whitelist.json", serde_json::json!({ "uuid": "b", "name": "Owner" }));
        add_list_entry(&dir, "ops.json", serde_json::json!({ "uuid": "b", "name": "Owner", "level": 4, "bypassesPlayerLimit": false }));
        assert_eq!(servers::names(&dir, "whitelist.json"), ["Friend", "Owner"]);
        assert_eq!(servers::names(&dir, "ops.json"), ["Owner"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn playit_is_off_unless_asked_for() {
        assert!(!servers::NewServer::default().playit);
    }

    #[test]
    fn playit_can_be_turned_off_and_stays_off() {
        let base = std::env::temp_dir().join(format!("hh-playit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let plugins = base.join("plugins");
        fs::create_dir_all(&plugins).unwrap();
        fs::write(plugins.join(PLAYIT), b"dll").unwrap();
        let server = base.join("server");
        fs::create_dir_all(server.join("plugins")).unwrap();
        // SAFETY: the only test that reads this variable.
        unsafe { std::env::set_var("HORIZON_HOST_PLUGINS", &plugins) };
        // A server HorizonX opens for the first time gets no public address of its own accord.
        adopt(&server);
        assert!(!playit_on(&server));
        // The switch itself (set_playit adds the privacy settings, which need a real server).
        switch_playit(&server, true).unwrap();
        assert!(playit_on(&server));
        switch_playit(&server, false).unwrap();
        assert!(!playit_on(&server));
        assert!(schedule(&server).playit_off);
        // Opening the server again doesn't put it back.
        adopt(&server);
        assert!(!playit_on(&server));
        switch_playit(&server, true).unwrap();
        assert!(playit_on(&server));
        assert!(!server.join("plugins").join(format!("{PLAYIT}.disabled")).exists());
        unsafe { std::env::remove_var("HORIZON_HOST_PLUGINS") };
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn java_layer_goes_in_with_a_jar() {
        let base = std::env::temp_dir().join(format!("hh-java-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let java = base.join("java");
        fs::create_dir_all(java.join("lib")).unwrap();
        fs::create_dir_all(java.join("runtime").join("bin")).unwrap();
        fs::write(java.join("lib").join("horizon-javalayer.jar"), b"jar").unwrap();
        fs::write(java.join(java_layer_file()), b"dll").unwrap();
        let server = base.join("server");
        fs::create_dir_all(server.join("plugins")).unwrap();
        let jar = base.join("Some.jar");
        fs::write(&jar, b"plugin").unwrap();
        // SAFETY: the only test that reads this variable.
        unsafe { std::env::set_var("HORIZON_HOST_JAVA", &java) };
        assert_eq!(add_jars(&server, &[jar]).unwrap(), 1);
        assert!(java_layer_on(&server));
        assert!(server.join("java-layer/lib/horizon-javalayer.jar").is_file());
        assert!(server.join("java-layer/runtime/bin").is_dir());
        // Again: nothing new to add.
        assert!(!ensure_java_layer(&server).unwrap());
        // Off: the layer goes aside and stays out, even when something asks for it again.
        set_java_layer(&server, false).unwrap();
        assert!(!java_layer_on(&server));
        assert!(schedule(&server).java_layer_off);
        assert!(!ensure_java_layer(&server).unwrap());
        assert!(!java_layer_on(&server));
        // On again.
        set_java_layer(&server, true).unwrap();
        assert!(java_layer_on(&server));
        assert!(!schedule(&server).java_layer_off);
        assert!(!server.join("plugins").join(format!("{}.disabled", java_layer_file())).exists());
        unsafe { std::env::remove_var("HORIZON_HOST_JAVA") };
        let _ = fs::remove_dir(server.join("java-layer/lib"));
        let _ = fs::remove_dir(server.join("java-layer/runtime"));
        let _ = fs::remove_dir_all(&base);
    }
}
