//! HorizonX updates itself. A newer build put in `update/` beside the app replaces the running one: the
//! app moves itself aside (Windows lets a running program be renamed, not overwritten), puts the new one in its
//! place, opens it and closes. Servers keep running: they are their own processes. The new copy says it was
//! updated and removes the old one.
use std::fs;
use std::path::PathBuf;

/// Set on the copy opened after an update.
pub const UPDATED: &str = "HORIZON_HOST_UPDATED";

fn paths() -> Option<(PathBuf, PathBuf, PathBuf)> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.to_path_buf();
    let name = exe.file_name()?.to_os_string();
    let stem = exe.file_stem()?.to_string_lossy().into_owned();
    let new = dir.join("update").join(&name);
    let old = dir.join(format!("{stem}.old{}", if cfg!(windows) { ".exe" } else { "" }));
    Some((exe, new, old))
}

/// A newer build waiting in `update/`.
pub fn pending() -> bool {
    let Some((exe, new, _)) = paths() else { return false };
    let (Ok(n), Ok(e)) = (fs::metadata(&new), fs::metadata(&exe)) else { return false };
    if !n.is_file() || n.len() < 1_000_000 {
        return false;
    }
    // A build still being copied in is younger than a few seconds: wait for it.
    let settled = n.modified().ok().and_then(|m| m.elapsed().ok()).is_some_and(|age| age.as_secs() >= 3);
    let newer = match (n.modified(), e.modified()) {
        (Ok(a), Ok(b)) => a != b || n.len() != e.len(),
        _ => n.len() != e.len(),
    };
    settled && newer
}

/// Puts the waiting build in place of this one and opens it. The caller closes this copy after.
pub fn apply() -> Result<(), String> {
    let (exe, new, old) = paths().ok_or("no app path")?;
    // The rest of a downloaded release first (engine, plugins, Java layer): what can't be replaced yet (a running
    // server holds it) waits for the next start.
    if let Some(dir) = exe.parent() {
        horizon_launcher::app_update::place_files(dir);
    }
    let _ = fs::remove_file(&old);
    fs::rename(&exe, &old).map_err(|e| format!("couldn't move the app aside: {e}"))?;
    if let Err(e) = fs::copy(&new, &exe) {
        let _ = fs::rename(&old, &exe);
        return Err(format!("couldn't put the update in: {e}"));
    }
    let _ = fs::remove_file(&new);
    std::process::Command::new(&exe)
        .current_dir(exe.parent().unwrap_or(std::path::Path::new(".")))
        .env(UPDATED, env!("CARGO_PKG_VERSION"))
        .spawn()
        .map_err(|e| format!("couldn't open the new version: {e}"))?;
    Ok(())
}

/// After an update: the old copy goes once it has closed. Returns whether this copy was just updated.
pub fn settle() -> bool {
    if let Some((exe, _, old)) = paths() {
        // The old copy may still be closing: try for a few seconds in the background.
        std::thread::spawn(move || {
            for _ in 0..20 {
                if !old.exists() || fs::remove_file(&old).is_ok() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            if let Some(dir) = exe.parent() {
                // Files of an update a running server held last time, and the old copies once nothing holds them.
                if dir.join("update").join("files").is_dir() {
                    horizon_launcher::app_update::place_files(dir);
                }
                horizon_launcher::app_update::clean_old(dir);
            }
        });
    }
    std::env::var_os(UPDATED).is_some()
}

// ----- From GitHub ------------------------------------------------------------------------------------------

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

static STATE: Mutex<String> = Mutex::new(String::new());
static GENERATION: AtomicU64 = AtomicU64::new(0);
static CHECKING: AtomicBool = AtomicBool::new(false);

/// What the updater is doing, for Settings ("Up to date", "Downloading HorizonX 0.3.1: 45%"...).
pub fn state() -> String {
    STATE.lock().map(|s| s.clone()).unwrap_or_default()
}

/// Changes whenever `state` does, so the window redraws.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

fn set(text: impl Into<String>) {
    if let Ok(mut s) = STATE.lock() {
        *s = text.into();
    }
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// An installed HorizonX (its engine beside it) updates itself; a build from source is updated with git.
pub fn online() -> bool {
    std::env::var_os("HORIZONX_NO_UPDATE").is_none()
        && std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("engines").join("horizonsx").is_dir())).unwrap_or(false)
}

/// Checks the releases now, in the background: a newer one is downloaded, checked and unpacked, and `pending`
/// then swaps to it.
pub fn check_now() {
    if !online() {
        set("Built from source: update it with git pull and a new build.");
        return;
    }
    if CHECKING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        check();
        CHECKING.store(false, Ordering::SeqCst);
    });
}

/// Checks soon after the app opens, then every 6 hours while it is open.
pub fn start_checking() {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(20));
        loop {
            check_now();
            std::thread::sleep(std::time::Duration::from_secs(6 * 3600));
        }
    });
}

fn check() {
    use horizon_launcher::app_update;
    let Some((exe, new, _)) = paths() else { return };
    let (Some(dir), Some(name)) = (exe.parent(), exe.file_name()) else { return };
    set("Checking for updates...");
    let found = match app_update::newer(env!("CARGO_PKG_VERSION")) {
        Ok(Some(f)) => f,
        Ok(None) => return set(format!("Up to date. Checked at {}.", chrono::Local::now().format("%H:%M"))),
        Err(e) => return set(format!("Couldn't check for updates: {e}")),
    };
    let update = dir.join("update");
    let version = found.version.clone();
    let shown = AtomicU64::new(u64::MAX);
    let zip = app_update::download(&found, &update, &|done, total| {
        let pct = if total > 0 { done * 100 / total } else { 0 };
        if shown.swap(pct, Ordering::Relaxed) != pct {
            set(format!("Downloading HorizonX {version}: {pct}%"));
        }
    });
    let zip = match zip {
        Ok(z) => z,
        Err(e) => return set(format!("Update to {version} failed: {e}")),
    };
    match app_update::stage(&zip, &update, &name.to_string_lossy()) {
        Ok(_) if new.is_file() => set(format!("HorizonX {version} is ready: it opens in a moment. Servers keep running.")),
        Ok(_) => set(format!("Update to {version} failed: the new app didn't unpack")),
        Err(e) => set(format!("Update to {version} failed: {e}")),
    }
}

/// `HorizonX update`: the same check, download and install, in the terminal. The app isn't reopened.
pub fn run_cli() -> i32 {
    if !online() {
        eprintln!("This copy isn't an installed HorizonX (no engines\\horizonsx beside it): update it with git pull and a build.");
        return 1;
    }
    println!("HorizonX {}: checking {} ...", env!("CARGO_PKG_VERSION"), horizon_launcher::app_update::REPO);
    check();
    let said = state();
    println!("{said}");
    let Some((exe, new, old)) = paths() else { return 1 };
    if !new.is_file() {
        return if said.starts_with("Up to date") { 0 } else { 1 };
    }
    let dir = exe.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
    let waiting = horizon_launcher::app_update::place_files(&dir);
    let _ = fs::remove_file(&old);
    if let Err(e) = fs::rename(&exe, &old).and_then(|_| fs::copy(&new, &exe).map(|_| ())) {
        let _ = fs::rename(&old, &exe);
        eprintln!("Couldn't put the new app in: {e}");
        return 1;
    }
    let _ = fs::remove_file(&new);
    if waiting > 0 {
        println!("Installed. {waiting} file(s) a running server holds go in when HorizonX next opens.");
    } else {
        println!("Installed. Open HorizonX again to use it; running servers take the new engine at their next restart.");
    }
    0
}
