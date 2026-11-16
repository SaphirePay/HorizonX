//! The network's dark menu window, for every plugin: the server pack (ZenithBackpacks' pack) draws a window for each
//! chest size (font `zbackpacks:gui`, `\u{E011}`..`\u{E016}` for 1-6 rows). Players who loaded the pack get it, with
//! the title in white; everyone else gets the game's own window. Was a copy in each plugin; now one module.
//!
//! ```ignore
//! horizon_kit::window::install(plugin);                 // on_enable
//! let mut m = horizon_kit::window::menu(&p, 6, "<b>Kits");
//! m.button(22, theme::item(...), |c| ...);
//! m.open(&p);
//! ```
//! `window::ensure(&p, reopen)` before opening sends the pack to someone who doesn't have it yet (once a session)
//! and opens the menu again in the window once it has loaded.

use crate::gui::Menu;
use crate::{app, mini};
use horizon_bukkit::fam_ev_player::PlayerResourcePackStatusEvent;
use horizon_bukkit::{Player, Plugin};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Instant;

/// The pack and its window glyphs.
#[derive(Clone, Debug)]
pub struct Style {
    pub pack_id: String,
    pub font: String,
    /// The glyph of a 1-row window; 2..6 rows follow it.
    pub first: u32,
    /// The space before and after the window glyph that lines it up with the chest.
    pub before: char,
    pub after: char,
    /// Off: everyone gets the game's own window.
    pub enabled: bool,
}

impl Default for Style {
    fn default() -> Self {
        Style { pack_id: "5a1f0c3e2b7d4c919e4a7b3d2f6a8c10".into(), font: "zbackpacks:gui".into(), first: 0xE011, before: '\u{E001}', after: '\u{E002}', enabled: true }
    }
}

static STYLE: Mutex<Option<Style>> = Mutex::new(None);
static LOADED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
type Reopen = Box<dyn Fn(&Player) + Send>;
static SENT: Mutex<Option<HashMap<String, (Instant, Option<Reopen>)>>> = Mutex::new(None);

fn style() -> Style {
    STYLE.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}

/// Another pack or glyphs (call before `install`).
pub fn set_style(s: Style) {
    *STYLE.lock().unwrap_or_else(|e| e.into_inner()) = Some(s);
}

/// Listens for the pack loading and remembers who has it. Call once in `on_enable` (and `save` in `on_disable`).
pub fn install(plugin: Plugin) {
    plugin.register_event_with("MONITOR", false, status);
    restore();
}

fn status(e: &mut PlayerResourcePackStatusEvent) {
    if !e.get_id().replace('-', "").eq_ignore_ascii_case(&style().pack_id) {
        return;
    }
    let Some(p) = e.get_player() else { return };
    let mut held = LOADED.lock().unwrap_or_else(|e| e.into_inner());
    let set = held.get_or_insert_with(HashSet::new);
    match e.get_status() {
        "SUCCESSFULLY_LOADED" => {
            set.insert(p.get_unique_id());
            drop(held);
            let reopen = SENT.lock().unwrap_or_else(|e| e.into_inner()).as_mut().and_then(|m| m.get_mut(&p.get_unique_id())).and_then(|(at, r)| {
                let fresh = at.elapsed().as_secs() < 30;
                r.take().filter(|_| fresh)
            });
            if let Some(r) = reopen {
                crate::sync::on_main(move || r(&p));
            }
        }
        "DECLINED" | "FAILED_DOWNLOAD" | "INVALID_URL" | "FAILED_RELOAD" | "DISCARDED" => {
            set.remove(&p.get_unique_id());
        }
        _ => {}
    }
}

/// Who already has the pack (after a reload): this plugin's saved list and ZenithBackpacks' one.
fn restore() {
    let mut text = std::fs::read_to_string(app::folder().join(".pack-loaded")).unwrap_or_default();
    if let Some(zb) = app::folder().parent().map(|f| f.join("ZenithBackpacks").join(".pack-loaded")) {
        text.push('\n');
        text.push_str(&std::fs::read_to_string(zb).unwrap_or_default());
    }
    let online: HashSet<String> = horizon_bukkit::get_online_players().iter().map(|p| p.get_unique_id()).collect();
    let set: HashSet<String> = text.lines().map(str::trim).filter(|u| online.contains(*u)).map(str::to_owned).collect();
    LOADED.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashSet::new).extend(set);
}

/// Remembers who has the pack across a reload (call in `on_disable`).
pub fn save() {
    let list: Vec<String> = LOADED.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|s| s.iter().cloned().collect()).unwrap_or_default();
    let _ = std::fs::write(app::folder().join(".pack-loaded"), list.join("\n"));
}

/// A player left.
pub fn forget(uuid: &str) {
    if let Some(s) = LOADED.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        s.remove(uuid);
    }
    if let Some(s) = SENT.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        s.remove(uuid);
    }
}

/// The pack ZenithBackpacks offers (its config's resource-pack url and sha1).
fn pack() -> Option<(String, String)> {
    let file = app::folder().parent()?.join("ZenithBackpacks").join("config.yml");
    let c = crate::config::Config::parse(&std::fs::read_to_string(file).ok()?);
    let url = c.str("resource-pack.url", "").trim().to_owned();
    let sha = c.str("resource-pack.sha1", "").trim().to_ascii_lowercase();
    (!url.is_empty() && sha.len() == 40).then_some((url, sha))
}

pub fn has(p: &Player) -> bool {
    style().enabled && LOADED.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|s| s.contains(&p.get_unique_id()))
}

/// Before a menu opens: a player without the pack gets it (once a session); `reopen` shows the menu again in the
/// window when it's in.
pub fn ensure(p: &Player, reopen: impl Fn(&Player) + Send + 'static) {
    let s = style();
    if !s.enabled || has(p) {
        return;
    }
    let uuid = p.get_unique_id();
    let mut sent = SENT.lock().unwrap_or_else(|e| e.into_inner());
    let sent = sent.get_or_insert_with(HashMap::new);
    if let Some((_, r)) = sent.get_mut(&uuid) {
        if r.is_some() {
            *r = Some(Box::new(reopen));
        }
        return;
    }
    let Some((url, sha)) = pack() else { return };
    sent.insert(uuid, (Instant::now(), Some(Box::new(reopen))));
    p.add_resource_pack(&s.pack_id, &url, &sha, None, false);
}

/// A menu title as JSON: with the pack, the dark window and the text in white; without it, the text as given.
pub fn title(p: &Player, rows: usize, mini_text: &str) -> String {
    let mut words: Value = mini::parse(mini_text);
    if !has(p) {
        if words.get("color").is_none() && words.is_object() {
            words["color"] = json!("#3f3f46");
        }
        return words.to_string();
    }
    let s = style();
    let frame = format!("{}{}{}", s.before, char::from_u32(s.first + rows.clamp(1, 6) as u32 - 1).unwrap_or(' '), s.after);
    json!({ "text": "", "extra": [{ "text": frame, "font": s.font, "color": "white" }, { "text": "", "color": "white", "extra": [words] }] }).to_string()
}

/// A themed menu for this player: the window (or the game's), every empty slot a clean black pane.
pub fn menu(p: &Player, rows: usize, mini_title: &str) -> Menu {
    let mut m = Menu::raw(rows, &title(p, rows, mini_title));
    m.fill(crate::theme::filler());
    m
}
