//! What Horizon's Rust plugins share, so a rewrite starts from working parts instead of a blank file. Made from
//! the DecentHolograms and Horizon World Edit rewrites:
//!
//! - [`text`]: `&` colours, hex, gradients, rainbows -> legacy `§` text and text components.
//! - [`fmt`]: Java `String.format` / `{0}` / `{player}` message filling.
//! - [`config`]: YAML config + messages files, defaults from the jar written on first start, typed getters.
//! - [`cmd`]: a command with sub-commands: permissions, usage, help page and tab completion from one table.
//! - [`packets`] + [`wire`]: entities only players' clients know (holograms, markers, NPCs); hot packets made as
//!   wire bytes (no JSON), sent in one ordered batch a tick, straight from a worker thread when they can be.
//! - [`worker`]: a plugin's tick on its own thread, as Java plugins run theirs async (server-thread cost ~0).
//! - [`stats`]: a `/x profiler` with named timers.
//! - [`mini`]: full MiniMessage (gradients, rainbow, hover/click, legacy codes mixed in) -> text JSON / legacy.
//! - [`hocon`]: HOCON (.conf, Configurate's format) read to JSON and written back.
//! - [`time`]: durations (`10m`, `2h30m`, `3 minutes`) parsed and printed.
//! - [`sync`]: `on_main` (back to the server thread), `hold`/`release` (keep an empty server ticking), `Cooldowns`.
//! - [`prelude`]: `use horizon_kit::prelude::*;` = the SDK prelude + kit modules + common helpers.
//! - [`app`]: the plugin handle in one static (`app::set/get/folder/info/later`).
//! - [`gui`]: chest menus (buttons, fillers, pages, click routing, storage menus) with one `install`.
//! - [`item`]: items from config sections (`Spec`: material/name/lore/model/enchants/flags/heads...), base64
//!   item text, names/lore as components, heads, enchantment/effect keys.
//! - [`store`]: `Docs` (one JSON per player/key, batched off-thread saves) and `File` (one JSON value).
//! - [`yaml`]: Bukkit `ConfigurationSection` reads over any YAML node (`string`, `list`, `list_or_one`, `int`,
//!   `is_list`, `is_section`, `keys`, `contains`).
//! - [`sound`]: Bukkit sound names -> keys, `NAME:vol:pitch`, `play`.
//! - [`hooks`]: economy, rank prefixes and placeholders across plugins (Vault/PlaceholderAPI without their jars).
//! - [`blocks`]: a box of blocks read in one query (`Snapshot`), a world's dimension id.
//! - [`ui`]: menus drawn by a resource pack: the art behind the slots and live text at any pixel ([`ui::Title`]),
//!   invisible buttons ([`gui::hotspot`]), slot animations ([`gui::animate`]), who has the pack ([`pack::ensure`]).
//!   Art and the pack: tools/uidraw.py, tools/uipack.py.
//! - [`client`]: for mods in single player: HUD drawing, key bindings, the crosshair's target, world outlines.
//! - [`theme`]: one look for every menu: the palette, item names, lore laid out the same way (`Lore`: fields,
//!   bars, "▸ Left-click to ..." lines), standard back/close/page/confirm/toggle buttons, a tooltip-free filler.
//! - [`window`]: the network's dark menu window from the server pack (with the game's window as the fallback),
//!   `window::menu(p, rows, title)` = a themed menu ready for buttons.
//! - [`dialog`]: the game's dialog screens with closures: notices, confirmations, text boxes, sliders, toggles,
//!   choices (`Dialog::new(..).input_text(..).button(.., |p, form| ..)`, `dialog::ask_number`, `dialog::confirm`).
//!
//! Recipe and rules: KIT.md beside this crate.

pub mod app;
pub mod blocks;
pub mod client;
pub mod cmd;
pub mod dialog;
pub mod yaml;
pub mod store;
pub mod sound;
pub mod inv;
pub mod item;
pub mod gui;
pub mod config;
pub mod fmt;
pub mod hocon;
pub mod hooks;
pub mod mini;
pub mod pack;
pub mod packets;
pub mod prelude;
pub mod screen;
pub mod smallcaps;
pub mod stats;
pub mod sync;
pub mod text;
pub mod theme;
pub mod time;
pub mod ui;
pub mod window;
mod ui_widths;
pub mod wire;
pub mod worker;

pub use cmd::{Ctx, Res, Router, Sub};
pub use config::{Config, Lang};
