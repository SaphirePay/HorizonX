//! Horizon Host updates itself. A newer build put in `update/` beside the app replaces the running one: the
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
    if let Some((_, _, old)) = paths() {
        // The old copy may still be closing: try for a few seconds in the background.
        std::thread::spawn(move || {
            for _ in 0..20 {
                if !old.exists() || fs::remove_file(&old).is_ok() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        });
    }
    std::env::var_os(UPDATED).is_some()
}
