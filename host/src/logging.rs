//! `HORIZONX_LOG=info` (error, warn, info, debug, trace): the app's and its renderer's messages on stderr, so a
//! problem on a PC we can't see (a window that stays black, a graphics card it can't use) can be reported. Off
//! unless set: nothing is printed or kept otherwise.
use log::{Level, LevelFilter, Log, Metadata, Record};

struct Stderr(LevelFilter);

impl Log for Stderr {
    fn enabled(&self, m: &Metadata) -> bool {
        m.level() <= self.0
    }

    fn log(&self, r: &Record) {
        if self.enabled(r.metadata()) {
            let level = match r.level() {
                Level::Error => "ERROR",
                Level::Warn => "WARN",
                Level::Info => "INFO",
                Level::Debug => "DEBUG",
                Level::Trace => "TRACE",
            };
            eprintln!("[{level} {}] {}", r.target(), r.args());
        }
    }

    fn flush(&self) {}
}

pub fn init() {
    let Some(level) = std::env::var("HORIZONX_LOG").ok().and_then(|v| v.trim().parse::<LevelFilter>().ok()) else { return };
    if log::set_boxed_logger(Box::new(Stderr(level))).is_ok() {
        log::set_max_level(level);
    }
}
