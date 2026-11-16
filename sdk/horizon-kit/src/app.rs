//! The plugin handle every rewrite keeps in a static, kept here once: `app::set(plugin)` in `on_enable`,
//! `app::get()` anywhere after, `app::folder()` for its data folder, `app::info/warn/severe` for the log.

use horizon_bukkit::Plugin;
use std::path::PathBuf;
use std::sync::Mutex;

static PLUGIN: Mutex<Option<Plugin>> = Mutex::new(None);

pub fn set(plugin: Plugin) {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(plugin);
}
/// Forget it (`on_disable`).
pub fn clear() {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}
pub fn get() -> Option<Plugin> {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner())
}
/// The plugin's data folder (made if missing); `plugins/<name>` before enable.
pub fn folder() -> PathBuf {
    get().map(|p| p.get_data_folder()).unwrap_or_else(|| PathBuf::from("plugins"))
}
pub fn info(msg: &str) {
    if let Some(p) = get() {
        p.info(msg);
    }
}
pub fn warn(msg: &str) {
    if let Some(p) = get() {
        p.warning(msg);
    }
}
pub fn severe(msg: &str) {
    if let Some(p) = get() {
        p.severe(msg);
    }
}
/// Runs `f` on the server thread `ticks` from now (0 = next tick).
pub fn later(ticks: u64, f: impl FnMut() + Send + 'static) {
    if let Some(p) = get() {
        if ticks == 0 {
            p.run_task(f);
        } else {
            p.run_task_later(ticks, f);
        }
    }
}
