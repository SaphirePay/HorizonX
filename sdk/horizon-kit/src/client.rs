//! The player's own game, for mods: HUD drawing, key bindings, what the crosshair is on, boxes and lines drawn in the
//! world, the frame rate and the GUI's size.
//!
//! A mod is a plugin DLL inside an `.hmod` (`native/windows-x64/<id>.dll`). In single player and on a LAN host the
//! game runs it on the integrated server's thread, and these calls reach the player's window through the mod
//! engine's queries (`mc_*`, the `modconv_client` bundle). Call them on the server thread: in an event, a command or
//! a timer (`plugin.run_task_timer`). On a dedicated server there is no window: they answer nothing and draw nothing.
//!
//! ```ignore
//! use horizon_kit::client::{self, Hud, Key};
//! let toggle = Key::register("key.timber.toggle", "key.keyboard.k", "key.categories.timber");
//! plugin.run_task_timer(1, 1, move || {
//!     if toggle.clicked() { /* flip a setting */ }
//!     Hud::new().text(4.0, 4.0, "Timber: ON", 0xFF55FF55).show("timber");
//! });
//! ```
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Mutex;

fn ask(request: Value) -> Value {
    horizon_bukkit::query_from_main(request)
}

/// Whether this game has a window the calls reach (single player or a LAN host on the mod engine).
pub fn available() -> bool {
    ask(json!({ "op": "mc_gui_size" }))["width"].is_number()
}

/// The HUD's size in GUI pixels (what `Hud` coordinates are in), as the last frame had it.
pub fn gui_size() -> (f32, f32) {
    let size = ask(json!({ "op": "mc_gui_size" }));
    (size["width"].as_f64().unwrap_or(320.0) as f32, size["height"].as_f64().unwrap_or(240.0) as f32)
}

/// Frames drawn in the last whole second.
pub fn fps() -> u32 {
    ask(json!({ "op": "mc_fps" })).as_u64().unwrap_or(0) as u32
}

/// The local player's jumps since the last call.
pub fn jumps() -> u32 {
    ask(json!({ "op": "mc_jump_poll" })).as_u64().unwrap_or(0) as u32
}

/// Mouse buttons pressed (true) or let go (false) since the last call: 0 left, 1 right, 2 middle, 3+ side buttons.
pub fn mouse() -> Vec<(u8, bool)> {
    ask(json!({ "op": "mc_mouse_poll" }))
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| (m["b"].as_u64().unwrap_or(0) as u8, m["a"].as_u64() == Some(1)))
        .collect()
}

/// Translations in the player's language: each key's text, None when the game has none.
pub fn lang(keys: &[&str]) -> HashMap<String, Option<String>> {
    let answer = ask(json!({ "op": "mc_lang", "keys": keys }));
    keys.iter().map(|k| (k.to_string(), answer[*k].as_str().map(str::to_owned))).collect()
}

/// What the crosshair is on.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Block { pos: [i32; 3], face: String },
    Entity { id: i64, kind: String, distance: f64 },
    Miss,
}

/// What the crosshair is on, as of the last frame.
pub fn crosshair() -> Target {
    let t = ask(json!({ "op": "mc_crosshair" }));
    match t["kind"].as_str() {
        Some("BLOCK") => {
            let p: Vec<i32> = t["block"]["pos"].as_array().into_iter().flatten().filter_map(|v| v.as_i64().map(|n| n as i32)).collect();
            if p.len() < 3 {
                return Target::Miss;
            }
            Target::Block { pos: [p[0], p[1], p[2]], face: t["block"]["face"].as_str().unwrap_or("UP").to_owned() }
        }
        Some("ENTITY") => {
            let e = &t["entity"];
            Target::Entity { id: e["id"].as_i64().unwrap_or(0), kind: e["type"].as_str().unwrap_or_default().to_owned(), distance: e["distance"].as_f64().unwrap_or(0.0) }
        }
        _ => Target::Miss,
    }
}

/// A key binding of the mod's, listed in Controls under "Converted mods" and saved with the player's other keys.
#[derive(Debug, Clone)]
pub struct Key {
    pub name: String,
}

impl Key {
    /// Registers a key once (a second registration of the same name is refused by the game and ignored here).
    /// `default` is a key name such as `key.keyboard.k`, `key.keyboard.left.alt` or `key.mouse.middle`.
    pub fn register(name: &str, default: &str, category: &str) -> Key {
        let _ = ask(json!({ "op": "mc_key_register", "name": name, "key": default, "category": category }));
        Key { name: name.to_owned() }
    }

    fn poll(&self, consume: bool) -> Value {
        ask(json!({ "op": "mc_key_poll", "name": self.name, "consume": consume }))
    }

    /// One press since the last call, used up (call it once a tick: each press counts once).
    pub fn clicked(&self) -> bool {
        self.poll(true)["click"].as_bool().unwrap_or(false)
    }

    /// Held down now.
    pub fn down(&self) -> bool {
        self.poll(false)["down"].as_bool().unwrap_or(false)
    }

    /// The key the player bound it to.
    pub fn bound(&self) -> String {
        self.poll(false)["bound"].as_str().unwrap_or_default().to_owned()
    }
}

/// What a mod draws on the HUD, in GUI pixels from the top left. `show` replaces the mod's last drawing; the game
/// keeps drawing it every frame until the next `show` (or `clear`), so send it only when it changes.
#[derive(Debug, Clone, Default)]
pub struct Hud {
    ops: Vec<Value>,
}

static LAST: Mutex<Vec<(String, Vec<Value>)>> = Mutex::new(Vec::new());

impl Hud {
    pub fn new() -> Hud {
        Hud::default()
    }

    /// A filled rectangle; `color` is ARGB (0x80000000 = half-see-through black).
    pub fn fill(mut self, x: f32, y: f32, w: f32, h: f32, color: u32) -> Hud {
        self.ops.push(json!({ "t": "fill", "x0": x, "y0": y, "x1": x + w, "y1": y + h, "c": color }));
        self
    }

    /// A rectangle shading from `top` to `bottom` (ARGB).
    pub fn gradient(mut self, x: f32, y: f32, w: f32, h: f32, top: u32, bottom: u32) -> Hud {
        self.ops.push(json!({ "t": "gradient", "x0": x, "y0": y, "x1": x + w, "y1": y + h, "c": top, "c2": bottom }));
        self
    }

    /// Text with a shadow; `§` colour codes work. 9 GUI pixels a line.
    pub fn text(self, x: f32, y: f32, text: &str, color: u32) -> Hud {
        self.text_with(x, y, &json!(text), color, true, 1.0)
    }

    /// Text from a text component (`{"text": .., "color": .., "bold": ..}` or a list), shadow and scale chosen.
    pub fn text_with(mut self, x: f32, y: f32, component: &Value, color: u32, shadow: bool, scale: f32) -> Hud {
        self.ops.push(json!({ "t": "text", "x": x, "y": y, "s": component, "c": color, "sh": shadow, "k": scale }));
        self
    }

    /// An item's icon (16x16), by id: `minecraft:diamond_axe`.
    pub fn item(mut self, x: f32, y: f32, id: &str) -> Hud {
        self.ops.push(json!({ "t": "item", "x": x, "y": y, "id": id }));
        self
    }

    /// Draws it for mod `owner` (its id), only when it differs from what that mod showed last.
    pub fn show(self, owner: &str) {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if last.iter().any(|(o, ops)| o == owner && *ops == self.ops) {
            return;
        }
        let _ = ask(json!({ "op": "mc_hud", "mod": owner, "ops": self.ops }));
        last.retain(|(o, _)| o != owner);
        last.push((owner.to_owned(), self.ops));
    }
}

/// Takes the mod's HUD drawing off.
pub fn clear(owner: &str) {
    Hud::new().show(owner);
}

/// A box or line drawn in the world, in block coordinates.
#[derive(Debug, Clone, Copy)]
pub struct Shape {
    pub min: [f64; 3],
    pub max: [f64; 3],
    /// ARGB.
    pub color: u32,
    pub width: f32,
    pub line: bool,
    /// Seen through blocks.
    pub on_top: bool,
}

impl Shape {
    /// The outline of block `pos`.
    pub fn block(pos: [i32; 3], color: u32) -> Shape {
        let min = [pos[0] as f64, pos[1] as f64, pos[2] as f64];
        Shape { min, max: [min[0] + 1.0, min[1] + 1.0, min[2] + 1.0], color, width: 2.0, line: false, on_top: false }
    }

    /// A box from `min` to `max`.
    pub fn cuboid(min: [f64; 3], max: [f64; 3], color: u32) -> Shape {
        Shape { min, max, color, width: 2.0, line: false, on_top: false }
    }

    /// A line from `a` to `b`.
    pub fn line(a: [f64; 3], b: [f64; 3], color: u32) -> Shape {
        Shape { min: a, max: b, color, width: 2.0, line: true, on_top: false }
    }

    pub fn through_walls(mut self) -> Shape {
        self.on_top = true;
        self
    }
}

/// Every shape the mods draw in the world now (one list for all of a game's mods: the last call wins).
pub fn shapes(shapes: &[Shape]) {
    let list: Vec<Value> = shapes
        .iter()
        .map(|s| json!({ "min": s.min, "max": s.max, "color": s.color, "width": s.width, "line": s.line, "on_top": s.on_top }))
        .collect();
    let _ = ask(json!({ "op": "mc_gizmos", "shapes": list }));
}
