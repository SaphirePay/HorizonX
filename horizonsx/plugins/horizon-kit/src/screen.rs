//! Screen glyphs: text displays only their player's client knows about, which a resource pack's core text shader
//! (UltimateUI's and packs like it) lifts out of the world onto the screen. A glyph whose vertices sit below y -1000 is drawn in a 1920x1080 frame:
//! x from the translation's `960 - x`, y from `-15000.178 + 540 - y`, depth from the layer.

use crate::packets::{self, BYTE, COMPONENT, INT, ITEM, QUATERNION, VECTOR3, entry};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Mutex;

/// Text displays scale a 64 px glyph to 1.6 blocks: page pixels to display scale.
const SIZE: f64 = 1.25;
/// The left rotation: a turn about y (the half turn the shader undoes, `pos.x *= -1`, less the player's yaw in a
/// world frame), then `deg` about the glyph's z.
pub fn left_rotation(deg: f32, frame: Frame) -> [f64; 4] {
    let a = match frame {
        Frame::Screen => std::f64::consts::PI,
        Frame::World { yaw, .. } => std::f64::consts::PI - f64::from(yaw).to_radians(),
    };
    let (sa, ca) = ((a / 2.0).sin(), (a / 2.0).cos());
    let (sb, cb) = ((f64::from(deg).to_radians() / 2.0).sin(), (f64::from(deg).to_radians() / 2.0).cos());
    [sa * sb, sa * cb, ca * sb, ca * cb]
}

/// Where a glyph lives: lifted to the screen by the pack's shader, or (no shader needed: packs like Iris replace the
/// core shaders) on a panel in the world facing the player, pixels shrunk by `divisor` and layers `gap` apart.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Frame {
    #[default]
    Screen,
    World { yaw: f32, divisor: f64, gap: f64 },
}
const FULL_BRIGHT: i64 = 15_728_880;

/// Where a box's glyph goes and how big, in page pixels (the original's box placement).
pub fn place_box(x: f64, y: f64, layer: f64, w: f64, h: f64) -> ([f64; 3], [f64; 3]) {
    let w = w.max(1.0);
    let h = h.max(1.0);
    let sw = w * SIZE;
    let sh = h * SIZE;
    let tall = ((h - 200.0) * 0.0032).max(0.0);
    let wide = if w <= 2876.0 { 0.0 } else { ((w - 2876.0) / 1920.0).floor() + 1.0 };
    ([x + w / 2.0 - sw / 160.0 + wide, y + h + sh / 36.0 - tall, layer], [sw / 2.0, sh / 2.0, 1.0])
}

/// The original's exact box placement (rounded blocks' strips and corners).
pub fn place_exact(x: f64, y: f64, layer: f64, w: f64, h: f64) -> ([f64; 3], [f64; 3]) {
    let w = w.max(1.0);
    let h = h.max(1.0);
    let sw = w * SIZE;
    let sh = h * SIZE;
    let wide = if w <= 2876.0 { 0.0 } else { ((w - 2876.0) / 1920.0).floor() + 1.0 };
    ([x + w / 2.0 - sw / 160.0 + wide, y + h + h * 0.03125, layer], [sw / 2.0, sh / 2.0, 1.0])
}

/// A text's top edge as the font draws it (glyphs sit on their ascent).
pub fn text_top(y: f64, h: f64) -> f64 {
    let h = h.max(1.0);
    y - (h - (h * 0.116_666_666_666_666_67).max(1.0))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Align {
    Left,
    #[default]
    Center,
    Right,
}

impl Align {
    /// The text display flag bits for it.
    pub fn flag(self) -> u8 {
        match self {
            Align::Left => 8,
            Align::Right => 16,
            Align::Center => 0,
        }
    }
}

/// A text box's anchor for its alignment.
pub fn align_x(x: f64, w: f64, align: Align) -> f64 {
    match align {
        Align::Left => x + w * 1.055,
        Align::Right => x - w * 1.055,
        Align::Center => x,
    }
}

/// Where a glyph pins on screens wider or narrower than 16:9 (the shader's bands below y -20000): the left edge,
/// the middle (the default) or the right edge.
pub fn anchor_offset(anchor: Align) -> f64 {
    match anchor {
        Align::Left => 10000.0,
        Align::Center => 20000.0,
        Align::Right => 30000.0,
    }
}

pub fn translation(at: [f64; 3], anchor: Align, frame: Frame) -> Value {
    match frame {
        Frame::Screen => json!([960.0 - at[0], -15000.178 + (540.0 - at[1]) - anchor_offset(anchor), -at[2] * 100.0]),
        Frame::World { yaw, divisor, gap } => {
            // The original's fix-shaders layers: the high ones (the editor's shell) pulled back.
            let z = if at[2] >= 6000.0 { at[2] - 7400.0 } else { at[2] };
            let (x, y, z) = ((960.0 - at[0]) / divisor, (540.0 - at[1]) / divisor, -z * gap);
            let r = -f64::from(yaw).to_radians();
            json!([x * r.cos() + z * r.sin(), y, -x * r.sin() + z * r.cos()])
        }
    }
}

fn scale_of(g: &Glyph) -> Value {
    match g.frame {
        Frame::Screen => json!(g.scale),
        Frame::World { divisor, .. } => json!([g.scale[0] / divisor, g.scale[1] / divisor, g.scale[2] / divisor]),
    }
}

/// A MiniMessage text's component, made once per text.
pub fn component(text: &str) -> Value {
    static CACHE: Mutex<Option<HashMap<String, Value>>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let map = cache.get_or_insert_with(HashMap::new);
    if let Some(hit) = map.get(text) {
        return hit.clone();
    }
    let made = crate::mini::parse_mixed(text);
    if map.len() > 4096 {
        map.clear();
    }
    map.insert(text.to_owned(), made.clone());
    made
}

/// One glyph as last sent, so updates send only what changed.
#[derive(Clone, PartialEq, Debug)]
pub struct Glyph {
    pub at: [f64; 3],
    pub scale: [f64; 3],
    pub text: String,
    pub opacity: u8,
    /// 0 centre, 8 left, 16 right (text display flags).
    pub align: u8,
    pub wrap: i32,
    /// Which screen edge it keeps to.
    pub anchor: Align,
    /// Turned about the screen's normal, in degrees.
    pub rot: f32,
    pub frame: Frame,
    /// An item to show instead of text (an item display: `{item, count, components}`).
    pub item: Option<Value>,
    /// Added to the depth after the layer (an item's model sits a little in front).
    pub nudge: f64,
    /// 0 fixed, 1 turns about y to the viewer, 3 turns all the way (a world panel's look-at).
    pub billboard: u8,
    /// A full left rotation (x, y, z, w) instead of `rot` (an item turned in three dimensions).
    pub turn: Option<[f64; 4]>,
    /// An item display's model transform (0 none, 2 third person right hand, 6 gui, 8 fixed).
    pub display: u8,
}

/// An item display's rotation: the frame's turn alone (the original gives items no half turn).
fn item_rotation(deg: f32, frame: Frame) -> [f64; 4] {
    let a = match frame {
        Frame::Screen => 0.0,
        Frame::World { yaw, .. } => -f64::from(yaw).to_radians(),
    };
    let (sa, ca) = ((a / 2.0).sin(), (a / 2.0).cos());
    let (sb, cb) = ((f64::from(deg).to_radians() / 2.0).sin(), (f64::from(deg).to_radians() / 2.0).cos());
    [sa * sb, sa * cb, ca * sb, ca * cb]
}

fn translation_of(g: &Glyph) -> Value {
    let mut t = translation(g.at, g.anchor, g.frame);
    if g.nudge != 0.0 {
        t[2] = json!(t[2].as_f64().unwrap_or(0.0) + g.nudge);
    }
    t
}

fn rotation_of(g: &Glyph) -> [f64; 4] {
    match g.turn {
        Some(q) => q,
        None if g.item.is_some() => item_rotation(g.rot, g.frame),
        None => left_rotation(g.rot, g.frame),
    }
}

fn opacity_byte(o: u8) -> Value {
    json!(o.max(5) as i8)
}

/// Everything a new glyph needs.
fn full(g: &Glyph) -> Vec<Value> {
    if let Some(item) = &g.item {
        return vec![
            entry(8, INT, json!(0)),
            entry(9, INT, json!(0)),
            entry(11, VECTOR3, translation_of(g)),
            entry(12, VECTOR3, scale_of(g)),
            entry(13, QUATERNION, json!(rotation_of(g))),
            entry(15, BYTE, json!(g.billboard)),
            entry(16, INT, json!(FULL_BRIGHT)),
            entry(23, ITEM, item.clone()),
            entry(24, BYTE, json!(g.display)),
        ];
    }
    vec![
        entry(8, INT, json!(0)),
        entry(9, INT, json!(0)),
        entry(11, VECTOR3, translation_of(g)),
        entry(12, VECTOR3, scale_of(g)),
        entry(13, QUATERNION, json!(rotation_of(g))),
        entry(15, BYTE, json!(g.billboard)),
        entry(16, INT, json!(FULL_BRIGHT)),
        entry(23, COMPONENT, component(&g.text)),
        entry(24, INT, json!(g.wrap)),
        entry(25, INT, json!(0)),
        entry(26, BYTE, opacity_byte(g.opacity)),
        entry(27, BYTE, json!(g.align)),
    ]
}

pub fn spawn(to: &[String], id: i32, at: [f64; 3], g: &Glyph) {
    packets::spawn(to, id, if g.item.is_some() { "minecraft:item_display" } else { "minecraft:text_display" }, at, 0.0);
    packets::data(to, id, full(g));
}

/// What changed between two states of a glyph, moving over `ticks` (the client smooths it).
pub fn update(to: &[String], id: i32, old: &Glyph, new: &Glyph, ticks: i32) {
    let mut out = Vec::new();
    let moved = old.at != new.at || old.scale != new.scale || old.anchor != new.anchor || old.rot != new.rot || old.frame != new.frame || old.nudge != new.nudge;
    if moved {
        out.push(entry(8, INT, json!(0)));
        out.push(entry(9, INT, json!(ticks.max(0))));
        out.push(entry(11, VECTOR3, translation_of(new)));
        out.push(entry(12, VECTOR3, scale_of(new)));
    }
    if old.rot != new.rot || old.frame != new.frame || old.turn != new.turn {
        out.push(entry(13, QUATERNION, json!(rotation_of(new))));
    }
    if old.billboard != new.billboard {
        out.push(entry(15, BYTE, json!(new.billboard)));
    }
    if let Some(item) = &new.item {
        if old.item.as_ref() != Some(item) {
            out.push(entry(23, ITEM, item.clone()));
        }
        if old.display != new.display {
            out.push(entry(24, BYTE, json!(new.display)));
        }
        packets::data(to, id, out);
        return;
    }
    if old.text != new.text {
        out.push(entry(23, COMPONENT, component(&new.text)));
    }
    if old.opacity != new.opacity {
        out.push(entry(26, BYTE, opacity_byte(new.opacity)));
    }
    if old.align != new.align {
        out.push(entry(27, BYTE, json!(new.align)));
    }
    if old.wrap != new.wrap {
        out.push(entry(24, INT, json!(new.wrap)));
    }
    packets::data(to, id, out);
}

/// An empty, still text display (the camera and the seat).
pub fn anchor(to: &[String], id: i32, at: [f64; 3], yaw: f32) {
    packets::spawn(to, id, "minecraft:text_display", at, yaw);
    packets::data(to, id, vec![entry(23, COMPONENT, json!({ "text": "" })), entry(25, INT, json!(0))]);
}

/// An invisible marker armor stand: a camera or a seat every client takes (a client may only let the camera follow
/// living entities; a marker's eye is at its feet, as a display's is).
pub fn stand(to: &[String], id: i32, at: [f64; 3], yaw: f32) {
    packets::spawn(to, id, "minecraft:armor_stand", at, yaw);
    packets::data(to, id, packets::stand_data(None, false, true));
}

/// An interaction box: the crosshair's ray lands on it, so both mouse buttons reach the server.
pub fn click_catcher(to: &[String], id: i32, at: [f64; 3], size: f32) {
    packets::spawn(to, id, "minecraft:interaction", at, 0.0);
    packets::data(to, id, vec![entry(8, 3, json!(size)), entry(9, 3, json!(size)), entry(10, 8, json!(false))]);
}

fn raw(name: &str, body: impl FnOnce(&mut crate::wire::W)) -> Option<Vec<u8>> {
    let id = crate::wire::packet_id(name)?;
    let mut w = crate::wire::W::default();
    w.varint(id);
    body(&mut w);
    let mut out = crate::wire::W::default();
    out.varint(w.0.len() as i32);
    out.0.extend_from_slice(&w.0);
    Some(out.0)
}

/// SetPassengers: who rides `vehicle` (all of them, in one list).
pub fn passengers(to: &[String], vehicle: i32, riders: &[i32]) {
    if let Some(bytes) = raw("SetPassengers", |w| {
        w.varint(vehicle);
        w.varint(riders.len() as i32);
        for r in riders {
            w.varint(*r);
        }
    }) {
        packets::queue_raw(to, bytes);
    } else {
        packets::queue(to, "SetPassengers", json!([["vehicle", vehicle], ["passengers", riders]]));
    }
}

/// SetCamera: the client looks from this entity (its own player's id to go back).
pub fn camera(to: &[String], entity: i32) {
    if let Some(bytes) = raw("SetCamera", |w| w.varint(entity)) {
        packets::queue_raw(to, bytes);
    } else {
        packets::queue(to, "SetCamera", json!([["camera_id", entity]]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_matches_the_original() {
        // calculateBoxPlacement(100, 200, 5, 64, 64): x + 32 - 80/160, y + 64 + 80/36.
        let (at, scale) = place_box(100.0, 200.0, 5.0, 64.0, 64.0);
        assert!((at[0] - 131.5).abs() < 1e-9 && (at[1] - (264.0 + 80.0 / 36.0)).abs() < 1e-9 && at[2] == 5.0);
        assert_eq!(scale, [40.0, 40.0, 1.0]);
        // A tall box drops by 0.0032 per pixel over 200.
        let (at, _) = place_box(0.0, 0.0, 0.0, 10.0, 1200.0);
        assert!((at[1] - (1200.0 + 1500.0 / 36.0 - 3.2)).abs() < 1e-9);
        assert_eq!(translation([960.0, 540.0, 2.0], Align::Center, Frame::Screen), json!([0.0, -35000.178, -200.0]));
        assert_eq!(translation([0.0, 0.0, 0.0], Align::Left, Frame::Screen), json!([960.0, -24460.178, -0.0]));
        // The screen frame's rotation is the half turn alone; a world frame facing yaw 0 is the same.
        let q = left_rotation(0.0, Frame::Screen);
        assert!((q[1] - 1.0).abs() < 1e-12 && q[0].abs() < 1e-12 && q[3].abs() < 1e-12);
        let w = Frame::World { yaw: 0.0, divisor: 1000.0, gap: 0.004 };
        assert_eq!(left_rotation(30.0, w), left_rotation(30.0, Frame::Screen));
        let t = translation([0.0, 540.0, 10.0], Align::Center, w);
        assert!((t[0].as_f64().unwrap() - 0.96).abs() < 1e-9 && t[1].as_f64().unwrap().abs() < 1e-9);
    }
}
