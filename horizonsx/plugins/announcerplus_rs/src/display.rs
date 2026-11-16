//! What players see over time: `{animate:...}` text animations, and the action bars, titles, boss bars and toasts
//! that run for a while (Java's `*UpdateTask`s, one async timer per display per player).
//!
//! Here one server-thread timer a tick walks every running display. A display re-parses its text only when an
//! animation frame changed it (placeholders at most once a second), and sends only what changed: a title with no
//! animation is one packet for its whole life instead of one a tick, an action bar is re-sent when it changes or
//! before it fades, a boss bar's name only when it changes.

use crate::{parse, plugin};
use horizon_bukkit::adventure::Component;
use horizon_bukkit::Player;
use horizon_kit::{mini, packets, wire};
use serde_json::{Value, json};
use std::sync::Mutex;

// ---- Animations ----

fn rand_u64() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEED: AtomicU64 = AtomicU64::new(0);
    let mut x = SEED.load(Ordering::Relaxed);
    if x == 0 {
        x = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(7, |d| d.as_nanos() as u64) | 1;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    SEED.store(x, Ordering::Relaxed);
    x
}

pub fn rand_f() -> f64 {
    (rand_u64() >> 11) as f64 / (1u64 << 53) as f64
}

fn hsv(h: f64, s: f64, v: f64) -> u32 {
    let h = h.rem_euclid(1.0) * 6.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let q = |f: f64| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u32;
    (q(r) << 16) | (q(g) << 8) | q(b)
}

fn random_color() -> u32 {
    hsv(rand_f(), 0.5 + rand_f() * 0.5, 0.5 + rand_f() * 0.5)
}

fn hex(rgb: u32) -> String {
    format!("#{rgb:06x}")
}

enum Anim {
    Flash { colors: Vec<String>, ticks: u32, index: usize, lived: u32 },
    Pulse { colors: Vec<u32>, step: f32, index: u32, color_index: usize, current: String },
    Scroll { inc: f32, phase: f32 },
    Type { text: String, ticks: u32, index: usize, lived: u32, underscore: bool },
    ScrollText { text: String, window: usize, ticks: u32, index: usize, lived: u32 },
}

impl Anim {
    fn create(tokens: &mut Vec<String>, kind: &str) -> Option<Anim> {
        let last_int = |t: &mut Vec<String>, d: u32| match t.last().and_then(|x| x.parse::<u32>().ok()) {
            Some(n) => {
                t.pop();
                n.max(1)
            }
            None => d,
        };
        Some(match kind {
            "flash" => {
                let ticks = last_int(tokens, 10);
                if tokens.is_empty() {
                    return None;
                }
                Anim::Flash { colors: tokens.clone(), ticks, index: 0, lived: 0 }
            }
            "pulse" => {
                let ticks = last_int(tokens, 10);
                let colors: Vec<u32> = tokens.iter().map(|c| mini::rgb_of(c).unwrap_or(0xFFFFFF)).collect();
                Self::pulse(colors, ticks)?
            }
            "randomcolor" => {
                let kind = tokens.first().map(|s| s.to_ascii_lowercase()).unwrap_or_else(|| "pulse".into());
                let ticks = tokens.get(1).and_then(|t| t.parse().ok()).unwrap_or(10u32).max(1);
                let colors: Vec<u32> = (0..128).map(|_| random_color()).collect();
                if kind == "flash" { Anim::Flash { colors: colors.into_iter().map(hex).collect(), ticks, index: 0, lived: 0 } } else { Self::pulse(colors, ticks)? }
            }
            "scroll" => Anim::Scroll { inc: tokens.first().and_then(|t| t.parse().ok()).unwrap_or(0.1), phase: -1.0 },
            "type" => Anim::Type { text: tokens.first()?.clone(), ticks: tokens.get(1).and_then(|t| t.parse().ok()).unwrap_or(6u32).max(1), index: 0, lived: 0, underscore: true },
            "scrolltext" => {
                let window = tokens.get(1).and_then(|t| t.parse().ok()).unwrap_or(10usize);
                let pad = " ".repeat(window);
                Anim::ScrollText {
                    text: format!("{pad}{}{pad}", tokens.first()?),
                    window,
                    ticks: tokens.get(2).and_then(|t| t.parse().ok()).unwrap_or(4u32).max(1),
                    index: 0,
                    lived: 0,
                }
            }
            _ => return None,
        })
    }

    fn pulse(mut colors: Vec<u32>, ticks: u32) -> Option<Anim> {
        let first = *colors.first()?;
        colors.push(first);
        Some(Anim::Pulse { colors, step: 1.0 / ticks as f32, index: 0, color_index: 0, current: hex(first) })
    }

    fn value(&self) -> String {
        match self {
            Anim::Flash { colors, index, .. } => colors[*index].clone(),
            Anim::Pulse { current, .. } => current.clone(),
            Anim::Scroll { phase, .. } => format!("{}", phase.clamp(-1.0, 1.0)),
            Anim::Type { text, index, underscore, .. } => {
                let chars: Vec<char> = text.chars().collect();
                let shown: String = chars[..(*index).min(chars.len())].iter().collect();
                format!("{shown}{}", if *underscore { "_" } else { " " })
            }
            Anim::ScrollText { text, window, index, .. } => text.chars().skip(*index).take(*window).collect(),
        }
    }

    fn next(&mut self) -> String {
        match self {
            Anim::Flash { colors, ticks, index, lived } => {
                *lived += 1;
                if *lived % *ticks == 0 {
                    *index = (*index + 1) % colors.len();
                }
            }
            Anim::Pulse { colors, step, index, color_index, current } => {
                if *step * *index as f32 > 1.0 {
                    *color_index += 1;
                    *index = 0;
                }
                let factor = *step * *index as f32;
                *index += 1;
                if *color_index + 1 > colors.len() - 1 {
                    *color_index = 0;
                }
                let (a, b) = (colors[*color_index], colors[*color_index + 1]);
                let ch = |c: u32, s: u32| ((c >> s) & 0xFF) as f32;
                let mix = |s: u32| ((ch(a, s) + factor * (ch(b, s) - ch(a, s))).round().clamp(0.0, 255.0) as u32) << s;
                *current = hex(mix(16) | mix(8) | mix(0));
            }
            Anim::Scroll { inc, phase } => {
                if *phase >= 1.0 {
                    *phase = -1.0;
                }
                *phase += *inc;
            }
            Anim::Type { text, ticks, index, lived, underscore } => {
                *lived += 1;
                if *lived % 5 == 0 {
                    *underscore = !*underscore;
                }
                if *lived % *ticks == 0 {
                    *index += 1;
                    if *index > text.chars().count() {
                        *index = 0;
                    }
                }
            }
            Anim::ScrollText { text, window, ticks, index, lived } => {
                *lived += 1;
                if *lived % *ticks == 0 {
                    *index += 1;
                    if *index > text.chars().count().saturating_sub(*window) {
                        *index = 0;
                    }
                }
            }
        }
        self.value()
    }
}

/// A text with its `{animate:...}` parts (`AnimationHolder`).
pub struct Animated {
    text: String,
    parts: Vec<(String, Anim)>,
}

impl Animated {
    pub fn new(text: &str) -> Self {
        let mut parts = Vec::new();
        let mut rest = text;
        while let Some(start) = rest.find("{animate:") {
            let after = &rest[start + 9..];
            let Some(end) = after.find('}') else { break };
            let whole = &rest[start..start + 9 + end + 1];
            let body = after[..end].trim_matches('/');
            let mut tokens: Vec<String> = body.split(':').map(str::to_owned).collect();
            let kind = tokens.remove(0).to_ascii_lowercase();
            if let Some(a) = Anim::create(&mut tokens, &kind)
                && !parts.iter().any(|(k, _)| k == whole)
            {
                parts.push((whole.to_owned(), a));
            }
            rest = &after[end + 1..];
        }
        Self { text: text.to_owned(), parts }
    }
    pub fn is_static(&self) -> bool {
        self.parts.is_empty()
    }
    /// The text with every animation one frame on.
    pub fn next(&mut self) -> String {
        let mut out = self.text.clone();
        for (k, a) in &mut self.parts {
            out = out.replace(k.as_str(), &a.next());
        }
        out
    }
    pub fn current(&self) -> String {
        let mut out = self.text.clone();
        for (k, a) in &self.parts {
            out = out.replace(k.as_str(), &a.value());
        }
        out
    }
}

/// Placeholders filled at most once a second for the same text (a display's text changes every frame only
/// where its animations are; the placeholders around them don't need a lookup per tick).
struct Parsed {
    input: String,
    output: Value,
    at: u64,
}

fn parsed(cache: &mut Option<Parsed>, player: &Player, text: &str, tick: u64) -> Value {
    if let Some(c) = cache
        && c.input == text
        && tick - c.at < 20
    {
        return c.output.clone();
    }
    let output = mini::parse(&parse(Some(player), text));
    *cache = Some(Parsed { input: text.to_owned(), output: output.clone(), at: tick });
    output
}

// ---- Displays ----

#[derive(Clone, Debug)]
pub struct ActionBar {
    pub seconds: i64,
    pub fade: bool,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct Title {
    pub fade_in: i64,
    pub stay: i64,
    pub fade_out: i64,
    pub title: String,
    pub subtitle: String,
}

#[derive(Clone, Debug)]
pub struct Bar {
    pub seconds: i64,
    pub text: String,
    pub color: String,
    pub overlay: String,
    pub fill: String,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub icon: String,
    pub enchanted: bool,
    pub model_data: i64,
    pub item_model: String,
    pub header: String,
    pub footer: String,
    pub frame: String,
}

impl ActionBar {
    pub fn enabled(&self) -> bool {
        !self.text.is_empty()
    }
}
impl Title {
    pub fn enabled(&self) -> bool {
        !(self.fade_in == 0 && self.stay == 0 && self.fade_out == 0) && (!self.title.is_empty() || !self.subtitle.is_empty())
    }
}
impl Bar {
    pub fn enabled(&self) -> bool {
        !self.text.is_empty()
    }
}
impl Toast {
    pub fn enabled(&self) -> bool {
        !self.header.is_empty() || !self.footer.is_empty()
    }
}

enum Kind {
    Action { fade: bool, text: Animated, cache: Option<Parsed>, last: Option<Value>, sent_at: u64 },
    Title { fade_in: i64, fade_out: i64, title: Animated, sub: Animated, ct: Option<Parsed>, cs: Option<Parsed>, last: Option<(Value, Value)>, sent_at: u64 },
    /// A boss bar as packets (`wire::boss`): only this player sees it, nothing goes in the world's saved bars.
    Bar { id: u128, overlay: i32, fill: String, text: Animated, color: Animated, cache: Option<Parsed>, last_title: Option<Value>, last_colour: i32, last_progress: f64 },
}

struct Running {
    player: Player,
    uuid: String,
    lived: u64,
    life: u64,
    kind: Kind,
}

static RUNNING: Mutex<Vec<Running>> = Mutex::new(Vec::new());
static TOASTS: Mutex<Vec<(Player, Toast)>> = Mutex::new(Vec::new());
static TICK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// One action bar and one title per player (a new one replaces the old: `DisplayTracker`); boss bars stack.
fn track(r: Running) {
    let mut all = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
    let same = |k: &Kind, o: &Kind| matches!((k, o), (Kind::Action { .. }, Kind::Action { .. }) | (Kind::Title { .. }, Kind::Title { .. }));
    if !matches!(r.kind, Kind::Bar { .. }) {
        all.retain(|o| !(o.uuid == r.uuid && same(&o.kind, &r.kind)));
    }
    all.push(r);
}

pub fn show_action_bar(player: &Player, a: &ActionBar) {
    track(Running {
        player: player.clone(),
        uuid: player.get_unique_id(),
        lived: 0,
        life: (a.seconds.max(0) * 20) as u64,
        kind: Kind::Action { fade: a.fade, text: Animated::new(&a.text), cache: None, last: None, sent_at: 0 },
    });
}

pub fn show_title(player: &Player, t: &Title) {
    track(Running {
        player: player.clone(),
        uuid: player.get_unique_id(),
        lived: 0,
        life: ((t.fade_in + t.stay).max(0) * 20) as u64,
        kind: Kind::Title { fade_in: t.fade_in, fade_out: t.fade_out, title: Animated::new(&t.title), sub: Animated::new(&t.subtitle), ct: None, cs: None, last: None, sent_at: 0 },
    });
}

pub fn show_bar(player: &Player, b: &Bar) {
    track(Running {
        player: player.clone(),
        uuid: player.get_unique_id(),
        lived: 0,
        life: (b.seconds.max(0) * 20) as u64,
        kind: Kind::Bar {
            id: wire::boss::new_id(),
            overlay: wire::boss::overlay_of(&b.overlay),
            fill: b.fill.to_ascii_uppercase(),
            text: Animated::new(&b.text),
            color: Animated::new(&b.color),
            cache: None,
            last_title: None,
            last_colour: -1,
            last_progress: -1.0,
        },
    });
}

pub fn show_toast(player: &Player, t: &Toast) {
    TOASTS.lock().unwrap_or_else(|e| e.into_inner()).push((player.clone(), t.clone()));
}

/// Returns false when the display is over.
fn step(r: &mut Running, tick: u64) -> bool {
    let over = r.lived >= r.life || !r.player.is_online();
    let lived = r.lived;
    match &mut r.kind {
        Kind::Action { fade, text, cache, last, sent_at } => {
            if over {
                if !*fade && r.player.is_online() {
                    r.player.send_action_bar(&Component::empty());
                }
                return false;
            }
            let t = text.next();
            let v = parsed(cache, &r.player, &t, tick);
            // An action bar fades after 3 s: re-sent when it changes or every 2 s.
            if last.as_ref() != Some(&v) || tick - *sent_at >= 40 {
                r.player.send_action_bar(&Component(v.clone()));
                *last = Some(v);
                *sent_at = tick;
            }
        }
        Kind::Title { fade_in, fade_out, title, sub, ct, cs, last, sent_at } => {
            let fi = (*fade_in).max(0) as u32 * 20;
            if over {
                if *fade_out != 0 && r.player.is_online() {
                    let (a, b) = (parsed(ct, &r.player, &title.current(), tick), parsed(cs, &r.player, &sub.current(), tick));
                    r.player.show_title(&Component(a), &Component(b), 0, 4, (*fade_out).max(0) as u32 * 20);
                }
                return false;
            }
            if title.is_static() && sub.is_static() {
                // Nothing moves: one title for its whole life.
                if lived == 0 {
                    let (a, b) = (parsed(ct, &r.player, &title.current(), tick), parsed(cs, &r.player, &sub.current(), tick));
                    let stay = (r.life as u32).saturating_sub(fi);
                    r.player.show_title(&Component(a), &Component(b), fi, stay, 0);
                }
            } else {
                let (ta, sa) = (title.next(), sub.next());
                let pair = (parsed(ct, &r.player, &ta, tick), parsed(cs, &r.player, &sa, tick));
                let first = lived == 0;
                let fading_in = fi > 0 && lived <= fi as u64;
                if first || (!fading_in && (last.as_ref() != Some(&pair) || tick - *sent_at >= 8)) {
                    r.player.show_title(&Component(pair.0.clone()), &Component(pair.1.clone()), if first { fi } else { 0 }, if first { fi + 10 } else { 10 }, 0);
                    *last = Some(pair);
                    *sent_at = tick;
                }
            }
        }
        Kind::Bar { id, overlay, fill, text, color, cache, last_title, last_colour, last_progress } => {
            let me = [r.uuid.clone()];
            if over {
                if let Some(bytes) = wire::boss::remove(*id) {
                    packets::queue_raw(&me, bytes);
                }
                return false;
            }
            let colour = wire::boss::colour_of(&color.next());
            let title = parsed(cache, &r.player, &text.next(), tick);
            let life = r.life.max(1) as f64;
            let progress = match fill.as_str() {
                "FILL" => lived as f64 / life,
                "FULL" => 1.0,
                "EMPTY" => 0.0,
                _ => 1.0 - lived as f64 / life,
            }
            .clamp(0.0, 1.0);
            if lived == 0 {
                if let Some(bytes) = wire::boss::add(*id, &title, progress as f32, colour, *overlay) {
                    packets::queue_raw(&me, bytes);
                }
                (*last_title, *last_colour, *last_progress) = (Some(title), colour, progress);
            } else {
                if last_title.as_ref() != Some(&title) {
                    if let Some(bytes) = wire::boss::title(*id, &title) {
                        packets::queue_raw(&me, bytes);
                    }
                    *last_title = Some(title);
                }
                if *last_colour != colour {
                    if let Some(bytes) = wire::boss::style(*id, colour, *overlay) {
                        packets::queue_raw(&me, bytes);
                    }
                    *last_colour = colour;
                }
                if (progress - *last_progress).abs() >= 0.004 {
                    if let Some(bytes) = wire::boss::progress(*id, progress as f32) {
                        packets::queue_raw(&me, bytes);
                    }
                    *last_progress = progress;
                }
            }
        }
    }
    r.lived += 1;
    true
}

fn toast_json(player: &Player, t: &Toast) -> String {
    let mut components = serde_json::Map::new();
    if t.enchanted {
        components.insert("minecraft:enchantment_glint_override".into(), json!(true));
    }
    if t.model_data >= 0 {
        components.insert("minecraft:custom_model_data".into(), json!({ "floats": [t.model_data as f32] }));
    }
    if !t.item_model.is_empty() && t.item_model != "announcerplus:disabled" {
        components.insert("minecraft:item_model".into(), json!(t.item_model));
    }
    let icon = t.icon.to_ascii_lowercase();
    let icon = if icon.contains(':') { icon } else { format!("minecraft:{icon}") };
    let title = json!({ "text": "", "extra": [mini::parse(&parse(Some(player), &t.header)), "\n", mini::parse(&parse(Some(player), &t.footer))] });
    json!({
        "criteria": { "impossible": { "trigger": "minecraft:impossible" } },
        "display": {
            "icon": { "id": icon, "components": components },
            "title": title,
            "description": "AnnouncerPlus Toast Description",
            "frame": t.frame.to_ascii_lowercase(),
            "announce_to_chat": false,
            "show_toast": true,
            "hidden": true,
            "background": "minecraft:gui/advancements/backgrounds/stone",
        }
    })
    .to_string()
}

/// Toasts are advancements given and taken back: loaded, awarded, and two ticks later revoked and removed.
fn toasts() {
    let queued = std::mem::take(&mut *TOASTS.lock().unwrap_or_else(|e| e.into_inner()));
    if queued.is_empty() {
        return;
    }
    let Some(p) = plugin() else { return };
    let unsafe_values = horizon_bukkit::core2::get_unsafe();
    let mut shown = Vec::new();
    for (player, t) in queued {
        if !player.is_online() {
            continue;
        }
        let key = horizon_bukkit::persistence::NamespacedKey::new(&p, &format!("toast_{}", rand_u64() % 1_000_000));
        if let Some(adv) = unsafe_values.load_advancement(&key, &toast_json(&player, &t)) {
            player.get_advancement_progress(&adv).award_criteria("impossible");
            shown.push((player, adv, key));
        }
    }
    if shown.is_empty() {
        return;
    }
    p.run_task_later(2, move || {
        let unsafe_values = horizon_bukkit::core2::get_unsafe();
        for (player, adv, key) in &shown {
            if player.is_online() {
                player.get_advancement_progress(adv).revoke_criteria("impossible");
            }
            unsafe_values.remove_advancement(key);
        }
    });
}

/// The tick: every running display one frame on, the queued toasts shown.
pub fn tick() {
    let tick = TICK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut running = std::mem::take(&mut *RUNNING.lock().unwrap_or_else(|e| e.into_inner()));
    if !running.is_empty() {
        running.retain_mut(|r| step(r, tick));
        // Displays started during this tick (a command, a broadcast) were pushed to the emptied list.
        let mut held = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
        running.append(&mut held);
        *held = running;
    }
    toasts();
    packets::flush();
}

/// Everything shown taken away (disable, reload).
pub fn stop_all() {
    let running = std::mem::take(&mut *RUNNING.lock().unwrap_or_else(|e| e.into_inner()));
    for r in running {
        if let Kind::Bar { id, .. } = r.kind
            && let Some(bytes) = wire::boss::remove(id)
        {
            packets::queue_raw(&[r.uuid.clone()], bytes);
        }
    }
    packets::flush();
    TOASTS.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

pub fn quit(uuid: &str) {
    let mut all = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
    // The player is gone: their client forgets the bars with them.
    all.retain(|r| r.uuid != uuid);
}

// ---- <center> (ChatCentering) ----

fn char_width(c: char, bold: bool) -> u32 {
    let w = match c {
        'i' | '!' | ',' | '.' | ':' | ';' | '|' | '\'' => 1,
        'l' | '`' => 2,
        'I' | 't' | '[' | ']' | '"' | '(' | ')' | '*' | '{' | '}' | ' ' => 3,
        'f' | 'k' | '<' | '>' => 4,
        '@' | '~' => 6,
        _ => 5,
    };
    w + 1 + u32::from(bold && c != ' ')
}

fn width(v: &Value, bold: bool) -> u32 {
    let bold = v["bold"].as_bool().unwrap_or(bold);
    let mut w: u32 = v["text"].as_str().map_or(0, |t| t.chars().map(|c| char_width(c, bold)).sum());
    for child in v["extra"].as_array().into_iter().flatten() {
        w += if let Some(s) = child.as_str() { s.chars().map(|c| char_width(c, bold)).sum() } else { width(child, bold) };
    }
    w
}

/// Spaces that put a chat line in the middle of the default chat box.
pub fn center_prefix(v: &Value) -> String {
    const CENTER_PX: u32 = 154;
    let half = width(v, false) / 2;
    if half >= CENTER_PX {
        return String::new();
    }
    " ".repeat(((CENTER_PX - half) / 4) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn animations() {
        let mut a = Animated::new("<{animate:flash:red:blue:2}>hi {animate:type:abc:1}");
        assert_eq!(a.parts.len(), 2);
        let f1 = a.next();
        assert!(f1.starts_with("<red>hi a"), "{f1}");
        let f2 = a.next();
        assert!(f2.starts_with("<blue>hi ab"), "{f2}");
        let mut s = Animated::new("{animate:scrolltext:Hello:3:1}");
        assert_eq!(s.next(), "  H");
        let mut p = Animated::new("{animate:pulse:#000000:#ffffff:2}");
        assert_eq!(p.next(), "#000000");
        assert_eq!(p.next(), "#808080");
        assert!(Animated::new("plain").is_static());
        assert_eq!(center_prefix(&json!({ "text": "" })).len(), 38);
    }
}
