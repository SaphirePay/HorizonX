//! One background thread that writes files in order (atomically, through a temporary file).

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};

enum Job {
    Write(PathBuf, String),
    Append(PathBuf, String),
}

static TX: Mutex<Option<Sender<Job>>> = Mutex::new(None);
static HANDLE: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

pub fn start() {
    let (tx, rx) = channel::<Job>();
    let handle = std::thread::Builder::new()
        .name("chestshop-io".into())
        .spawn(move || {
            for job in rx {
                match job {
                    Job::Write(path, text) => {
                        if let Some(dir) = path.parent() {
                            let _ = std::fs::create_dir_all(dir);
                        }
                        let tmp = path.with_extension("tmp");
                        if std::fs::write(&tmp, text).is_ok() {
                            let _ = std::fs::rename(&tmp, &path);
                        }
                    }
                    Job::Append(path, text) => {
                        use std::io::Write;
                        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                            let _ = f.write_all(text.as_bytes());
                        }
                    }
                }
            }
        })
        .ok();
    *TX.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    *HANDLE.lock().unwrap_or_else(|e| e.into_inner()) = handle;
}

fn send(job: Job) {
    if let Some(tx) = TX.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = tx.send(job);
    }
}

pub fn write(path: PathBuf, text: String) {
    send(Job::Write(path, text));
}

pub fn append(path: PathBuf, text: String) {
    send(Job::Append(path, text));
}

/// Everything queued written, the thread ended.
pub fn stop() {
    *TX.lock().unwrap_or_else(|e| e.into_inner()) = None;
    if let Some(h) = HANDLE.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = h.join();
    }
}
