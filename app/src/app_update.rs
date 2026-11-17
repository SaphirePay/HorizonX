//! HorizonX keeps itself current from its GitHub releases, so nobody downloads the zip twice: the newest release
//! of `SaphirePay/HorizonX` is checked now and then; a newer one is downloaded, checked against its SHA-256, and
//! unpacked into `update/` beside the app. The app (`host/src/update.rs`) then swaps itself and puts the rest in
//! place: the engine, the plugins, the Java layer. Running servers keep running and take the new engine at their
//! next restart.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where the releases are.
pub const REPO: &str = "SaphirePay/HorizonX";

/// A release newer than this copy.
#[derive(Clone, Debug)]
pub struct Found {
    pub version: String,
    pub asset: String,
    pub url: String,
    pub size: u64,
    /// Lowercase hex.
    pub sha256: String,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(120))
        .user_agent(concat!("HorizonX/", env!("CARGO_PKG_VERSION"), " (updater)"))
        .build()
}

/// The release asset this PC runs: `HorizonX-<version>-windows-x64.zip` or `HorizonX-<version>-linux-x64.zip`.
fn asset_suffix() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("-windows-x64.zip"),
        ("linux", "x86_64") => Some("-linux-x64.zip"),
        _ => None,
    }
}

/// The app's own file in a release: `HorizonX.exe` on Windows, `HorizonX` elsewhere (any case).
fn is_app_file(rel: &Path) -> bool {
    if rel.components().count() != 1 || !rel.file_stem().is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case("horizonx")) {
        return false;
    }
    match rel.extension() {
        Some(e) => e.eq_ignore_ascii_case("exe"),
        None => !cfg!(windows),
    }
}

fn version_of(tag: &str) -> Option<semver::Version> {
    semver::Version::parse(tag.trim().trim_start_matches(['v', 'V'])).ok()
}

/// The latest release, when it is newer than `current` and has a build for this PC with a SHA-256.
pub fn newer(current: &str) -> Result<Option<Found>, String> {
    let Some(suffix) = asset_suffix() else { return Ok(None) };
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let release: Value = agent()
        .get(&url)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("Couldn't reach GitHub: {e}"))?
        .into_json()
        .map_err(|e| e.to_string())?;
    let tag = release["tag_name"].as_str().unwrap_or_default();
    let (Some(latest), Some(mine)) = (version_of(tag), version_of(current)) else { return Ok(None) };
    if latest <= mine {
        return Ok(None);
    }
    let assets = release["assets"].as_array().cloned().unwrap_or_default();
    let name_of = |a: &Value| a["name"].as_str().unwrap_or_default().to_string();
    let Some(zip) = assets.iter().find(|a| {
        let n = name_of(a);
        n.starts_with("HorizonX-") && n.ends_with(suffix)
    }) else {
        return Ok(None);
    };
    let asset = name_of(zip);
    // GitHub's own digest of the asset, else a `<asset>.sha256` beside it.
    let mut sha256 = zip["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).map(str::to_lowercase);
    if sha256.is_none() {
        if let Some(side) = assets.iter().find(|a| name_of(a) == format!("{asset}.sha256")) {
            let text = agent()
                .get(side["browser_download_url"].as_str().unwrap_or_default())
                .call()
                .map_err(|e| e.to_string())?
                .into_string()
                .map_err(|e| e.to_string())?;
            sha256 = text.split_whitespace().next().map(str::to_lowercase);
        }
    }
    let Some(sha256) = sha256.filter(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit())) else {
        // Never install something that can't be checked.
        return Ok(None);
    };
    Ok(Some(Found {
        version: latest.to_string(),
        asset,
        url: zip["browser_download_url"].as_str().unwrap_or_default().to_string(),
        size: zip["size"].as_u64().unwrap_or(0),
        sha256,
    }))
}

/// Downloads the release into `update/` and checks it. `progress(done, total)` as it goes.
pub fn download(found: &Found, update_dir: &Path, progress: &dyn Fn(u64, u64)) -> Result<PathBuf, String> {
    fs::create_dir_all(update_dir).map_err(|e| e.to_string())?;
    let target = update_dir.join(&found.asset);
    let part = update_dir.join(format!("{}.part", found.asset));
    let mut reader = agent().get(&found.url).call().map_err(|e| format!("Downloading {}: {e}", found.asset))?.into_reader();
    let mut out = fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("Downloading {}: {e}", found.asset))?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        progress(done, found.size);
    }
    drop(out);
    let got = hex(&hash.finalize());
    if got != found.sha256 {
        let _ = fs::remove_file(&part);
        return Err(format!("{} didn't match its SHA-256 (got {got}): not installed", found.asset));
    }
    fs::rename(&part, &target).map_err(|e| e.to_string())?;
    Ok(target)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Unpacks a downloaded release: the app itself to `update/<exe_name>` (where the app's self-update takes it from),
/// everything else to `update/files/` (put in place next to the app when it swaps). The zip's top folder goes.
pub fn stage(zip_path: &Path, update_dir: &Path, exe_name: &str) -> Result<usize, String> {
    let file = fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let files = update_dir.join("files");
    let _ = fs::remove_dir_all(&files);
    let mut app = None;
    let mut count = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(path) = entry.enclosed_name() else { continue };
        // Drop the top folder (HorizonX/...).
        let rel: PathBuf = path.components().skip(1).collect();
        if rel.as_os_str().is_empty() || entry.is_dir() {
            continue;
        }
        let is_app = is_app_file(&rel);
        #[cfg(unix)]
        let mode = entry.unix_mode();
        let to = if is_app { update_dir.join(format!("{exe_name}.part")) } else { files.join(&rel) };
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = fs::File::create(&to).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        // Linux: the engine, the app and the Java runtime's programs keep their executable bit (the zip carries
        // each file's mode; the app is always executable).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = mode.map(|m| m & 0o777).filter(|m| *m != 0).unwrap_or(0o644) | if is_app { 0o755 } else { 0 };
            fs::set_permissions(&to, fs::Permissions::from_mode(mode)).map_err(|e| e.to_string())?;
        }
        if is_app {
            app = Some(to);
        } else {
            count += 1;
        }
    }
    let app = app.ok_or(if cfg!(windows) { "The release has no HorizonX.exe" } else { "The release has no HorizonX program" })?;
    // Last, so the app's swap never sees half of it.
    fs::rename(&app, update_dir.join(exe_name)).map_err(|e| e.to_string())?;
    let _ = fs::remove_file(zip_path);
    Ok(count)
}

/// Puts the unpacked files from `update/files/` next to the app. A file a running server holds (its engine, a
/// library) is moved aside first, which Windows allows; one that can't be replaced stays waiting for the next try.
/// Returns how many are still waiting.
pub fn place_files(app_dir: &Path) -> usize {
    let staged = app_dir.join("update").join("files");
    let mut waiting = 0;
    for src in walk(&staged) {
        let Ok(rel) = src.strip_prefix(&staged) else { continue };
        let target = app_dir.join(rel);
        if same(&src, &target) {
            let _ = fs::remove_file(&src);
            continue;
        }
        if let Some(parent) = target.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let placed = fs::rename(&src, &target).is_ok() || {
            // In use: move it aside (removed at the next start), then put the new one in.
            let aside = target.with_file_name(format!("{}.hxold", target.file_name().unwrap_or_default().to_string_lossy()));
            let _ = fs::remove_file(&aside);
            fs::rename(&target, &aside).is_ok() && fs::rename(&src, &target).is_ok()
        };
        if !placed {
            waiting += 1;
        }
    }
    if waiting == 0 {
        let _ = fs::remove_dir_all(&staged);
    }
    waiting
}

/// The `.hxold` copies left by `place_files` once nothing holds them.
pub fn clean_old(app_dir: &Path) {
    for dir in ["engines", "plugins", "java"] {
        for f in walk(&app_dir.join(dir)) {
            if f.extension().is_some_and(|e| e == "hxold") {
                let _ = fs::remove_file(&f);
            }
        }
    }
    for f in fs::read_dir(app_dir).into_iter().flatten().flatten().map(|e| e.path()) {
        if f.extension().is_some_and(|e| e == "hxold") {
            let _ = fs::remove_file(&f);
        }
    }
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            // Linked folders (a server's java-layer/lib points at the app's) are not walked into.
            match e.file_type() {
                Ok(t) if t.is_dir() => todo.push(p),
                Ok(t) if t.is_file() => out.push(p),
                _ => {}
            }
        }
    }
    out
}

/// Same size and the same bytes.
pub fn same(a: &Path, b: &Path) -> bool {
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(x), Ok(y)) if x.len() == y.len() => matches!((fs::read(a), fs::read(b)), (Ok(p), Ok(q)) if p == q),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_semver() {
        assert!(version_of("v0.10.0").unwrap() > version_of("0.9.3").unwrap());
        assert!(version_of("v0.3.0").unwrap() > version_of("0.2.1").unwrap());
        assert!(version_of("nightly").is_none());
    }

    #[test]
    fn a_release_is_staged_and_placed() {
        let base = std::env::temp_dir().join(format!("hx-upd-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let app = base.join("app");
        let update = app.join("update");
        fs::create_dir_all(app.join("engines/horizonsx")).unwrap();
        fs::write(app.join("engines/horizonsx/horizon-client.exe"), b"old engine").unwrap();
        fs::write(app.join("README.txt"), b"same").unwrap();
        // A release zip: HorizonX/HorizonX.exe, HorizonX/engines/horizonsx/horizon-client.exe, HorizonX/README.txt.
        fs::create_dir_all(&update).unwrap();
        let zip_path = update.join("HorizonX-9.9.9-windows-x64.zip");
        {
            let mut w = zip::ZipWriter::new(fs::File::create(&zip_path).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            w.start_file("HorizonX/HorizonX.exe", o).unwrap();
            w.write_all(b"new app").unwrap();
            w.start_file("HorizonX/engines/horizonsx/horizon-client.exe", o).unwrap();
            w.write_all(b"new engine").unwrap();
            w.start_file("HorizonX/README.txt", o).unwrap();
            w.write_all(b"same").unwrap();
            w.finish().unwrap();
        }
        assert_eq!(stage(&zip_path, &update, "horizon-host.exe").unwrap(), 2);
        // The app goes under the running copy's own name, for the swap.
        assert_eq!(fs::read(update.join("horizon-host.exe")).unwrap(), b"new app");
        assert!(!zip_path.exists());
        assert_eq!(place_files(&app), 0);
        assert_eq!(fs::read(app.join("engines/horizonsx/horizon-client.exe")).unwrap(), b"new engine");
        assert_eq!(fs::read(app.join("README.txt")).unwrap(), b"same");
        assert!(!update.join("files").exists());
        let _ = fs::remove_dir_all(&base);
    }

    /// The Linux zip: the app has no `.exe`, and programs keep their executable bit through staging and placing.
    #[cfg(unix)]
    #[test]
    fn a_linux_release_keeps_modes() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("hx-upd-linux-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let app = base.join("app");
        let update = app.join("update");
        fs::create_dir_all(&update).unwrap();
        let zip_path = update.join("HorizonX-9.9.9-linux-x64.zip");
        {
            let mut w = zip::ZipWriter::new(fs::File::create(&zip_path).unwrap());
            let exe = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
            let text = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
            w.start_file("HorizonX/HorizonX", text).unwrap();
            w.write_all(b"new app").unwrap();
            w.start_file("HorizonX/engines/horizonsx/horizon-client", exe).unwrap();
            w.write_all(b"new engine").unwrap();
            w.start_file("HorizonX/plugins/catalog.json", text).unwrap();
            w.write_all(b"[]").unwrap();
            w.finish().unwrap();
        }
        assert!(is_app_file(Path::new("HorizonX")));
        assert!(!is_app_file(Path::new("engines/HorizonX")));
        assert_eq!(stage(&zip_path, &update, "HorizonX").unwrap(), 2);
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        // The app is always executable, even from a zip that lost its mode.
        assert_eq!(mode(&update.join("HorizonX")), 0o755);
        assert_eq!(place_files(&app), 0);
        assert_eq!(mode(&app.join("engines/horizonsx/horizon-client")), 0o755);
        assert_eq!(mode(&app.join("plugins/catalog.json")), 0o644);
        let _ = fs::remove_dir_all(&base);
    }
}
