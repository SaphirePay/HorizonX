//! `/chunky trim`: deletes the chunks outside (or inside) a selection from the world's region files, on its own
//! thread. A chunk is removed by clearing its entry in the region header (Anvil's 4 KiB location table), as Chunky
//! does; a region left empty is deleted. `entities/` and `poi/` get the same. The server must be restarted after.

use crate::{Selection, shape::Shape, tr};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[derive(Clone, Debug)]
pub struct Job {
    pub sel: Selection,
    pub inside: bool,
    dirs: Vec<PathBuf>,
}

static RUNNING: AtomicBool = AtomicBool::new(false);
static STOP: AtomicBool = AtomicBool::new(false);
static REGIONS_DONE: AtomicU64 = AtomicU64::new(0);
static REGIONS_TOTAL: AtomicU64 = AtomicU64::new(0);
/// Lines for the console, written from the server thread (`report`).
static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The 1024 chunk locations of a region file (0 = no chunk), or None when there's no such file.
pub fn read_header(path: &Path) -> Option<Box<[u32; 1024]>> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = [0u8; 4096];
    f.read_exact(&mut buf).ok()?;
    let mut out = Box::new([0u32; 1024]);
    for (i, c) in buf.chunks_exact(4).enumerate() {
        out[i] = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
    }
    Some(out)
}

/// `trim [world] [shape] [centerX] [centerZ] [radius] [radiusZ] [outside|inside]`, from the current selection.
pub fn parse(current: &Selection, a: &[&str]) -> Result<Job, ()> {
    let mut sel = current.clone();
    if let Some(w) = a.first() {
        sel.world = crate::world_arg(w).ok_or(())?.get_name().to_owned();
    }
    if let Some(sh) = a.get(1) {
        let sh = sh.to_ascii_lowercase();
        if !crate::SHAPES.contains(&sh.as_str()) && sh != "oval" {
            return Err(());
        }
        sel.shape = sh;
    }
    let num = |i: usize| a.get(i).map(|v| v.parse::<f64>().map_err(|_| ())).transpose();
    if let (Some(x), Some(z)) = (num(2)?, num(3)?) {
        (sel.center_x, sel.center_z) = (x, z);
    }
    if let Some(r) = num(4)? {
        (sel.radius_x, sel.radius_z) = (r, r);
    }
    if let Some(r) = num(5)? {
        sel.radius_z = r;
    }
    let inside = a.get(6).is_some_and(|m| m.eq_ignore_ascii_case("inside"));
    let world = horizon_bukkit::get_world(&sel.world).ok_or(())?;
    let dirs = ["region", "entities", "poi"].iter().map(|k| crate::region_dir(&world, k)).collect();
    Ok(Job { sel, inside, dirs })
}

/// Clears the listed chunks' entries in one region file; deletes the file when nothing is left. Returns how many
/// chunks were removed.
fn trim_file(path: &Path, delete: &dyn Fn(i32, i32) -> bool, rx: i32, rz: i32) -> std::io::Result<u64> {
    let Some(header) = read_header(path) else { return Ok(0) };
    let mut removed = 0;
    let mut left = 0;
    let mut cleared = Vec::new();
    for (i, loc) in header.iter().enumerate() {
        if *loc == 0 {
            continue;
        }
        let (x, z) = ((rx << 5) + (i as i32 & 31), (rz << 5) + (i as i32 >> 5));
        if delete(x, z) {
            cleared.push(i);
            removed += 1;
        } else {
            left += 1;
        }
    }
    if removed == 0 {
        return Ok(0);
    }
    if left == 0 {
        std::fs::remove_file(path)?;
    } else {
        let mut f = std::fs::OpenOptions::new().write(true).open(path)?;
        for i in cleared {
            f.seek(SeekFrom::Start(i as u64 * 4))?;
            f.write_all(&[0; 4])?;
            f.seek(SeekFrom::Start(4096 + i as u64 * 4))?;
            f.write_all(&[0; 4])?;
        }
    }
    // Oversized chunks live beside the region as c.X.Z.mcc.
    if let Some(dir) = path.parent() {
        let _ = header;
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(rest) = name.strip_prefix("c.").and_then(|n| n.strip_suffix(".mcc")) {
                let mut p = rest.split('.');
                if let (Some(Ok(x)), Some(Ok(z))) = (p.next().map(str::parse::<i32>), p.next().map(str::parse::<i32>))
                    && x >> 5 == rx
                    && z >> 5 == rz
                    && delete(x, z)
                {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
    Ok(removed)
}

pub fn run(job: Job) {
    if RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    STOP.store(false, Ordering::Release);
    let world = job.sel.world.clone();
    let _ = std::thread::Builder::new().name("Chunky-trim".into()).spawn(move || {
        let started = std::time::Instant::now();
        let shape = Shape::new(&job.sel);
        let inside = job.inside;
        let delete = move |x: i32, z: i32| shape.is_bounding(((x << 4) + 8) as f64, ((z << 4) + 8) as f64) == inside;
        let mut files: Vec<(PathBuf, i32, i32)> = Vec::new();
        for dir in &job.dirs {
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                let Some(rest) = name.strip_prefix("r.").and_then(|n| n.strip_suffix(".mca")) else { continue };
                let mut p = rest.split('.');
                if let (Some(Ok(x)), Some(Ok(z))) = (p.next().map(str::parse::<i32>), p.next().map(str::parse::<i32>)) {
                    files.push((e.path(), x, z));
                }
            }
        }
        REGIONS_TOTAL.store(files.len() as u64, Ordering::Release);
        REGIONS_DONE.store(0, Ordering::Release);
        let mut removed = 0u64;
        for (path, rx, rz) in files {
            if STOP.load(Ordering::Acquire) {
                break;
            }
            // Region-file chunks only count once (entities/poi hold the same chunks).
            let n = trim_file(&path, &delete, rx, rz).unwrap_or(0);
            if path.parent().and_then(|p| p.file_name()).is_some_and(|n| n == "region") {
                removed += n;
            }
            REGIONS_DONE.fetch_add(1, Ordering::AcqRel);
        }
        LINES.lock().unwrap_or_else(|e| e.into_inner()).push(tr("task_trim", true, &[removed.to_string(), world, format!("{:.1}", started.elapsed().as_secs_f64())]));
        RUNNING.store(false, Ordering::Release);
    });
}

/// Once a second on the server thread: trim progress and its final line to the console.
pub fn report() {
    let lines = std::mem::take(&mut *LINES.lock().unwrap_or_else(|e| e.into_inner()));
    for l in lines {
        horizon_bukkit::get_console_sender().send_message(&l);
    }
    if RUNNING.load(Ordering::Acquire) {
        let (done, total) = (REGIONS_DONE.load(Ordering::Acquire), REGIONS_TOTAL.load(Ordering::Acquire).max(1));
        let percent = format!("{:.2}", 100.0 * done as f64 / total as f64);
        horizon_bukkit::get_console_sender().send_message(&tr("task_trim_update", true, &["trim".into(), done.to_string(), percent]));
    }
}

pub fn stop() {
    STOP.store(true, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clears_header_entries() {
        let dir = std::env::temp_dir().join(format!("chunky_trim_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("r.0.0.mca");
        let mut data = vec![0u8; 8192 + 4096];
        // Chunks (0,0) and (1,0) present.
        data[0..4].copy_from_slice(&0x0000_0201u32.to_be_bytes());
        data[4..8].copy_from_slice(&0x0000_0301u32.to_be_bytes());
        std::fs::write(&path, &data).unwrap();
        let removed = trim_file(&path, &|x, _| x == 1, 0, 0).unwrap();
        assert_eq!(removed, 1);
        let h = read_header(&path).unwrap();
        assert_ne!(h[0], 0);
        assert_eq!(h[1], 0);
        let removed = trim_file(&path, &|_, _| true, 0, 0).unwrap();
        assert_eq!(removed, 1);
        assert!(!path.exists(), "an emptied region is deleted");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
