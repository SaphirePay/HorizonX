//! What a waiting player sees: the game's own dialog screen over black, drawn by the proxy (their place, the wait,
//! their pass if they have one, Get Priority and Leave Queue), and the words and numbers on it and in the server list.

use crate::queue::{QueueConfig, Standing};
use horizon_world::nbt::Tag;
use serde_json::{Value, json};

/// JSON (as text components and dialogs are written) -> the NBT the game reads them as in packets.
pub fn tag(v: &Value) -> Tag {
    match v {
        Value::Null => Tag::String("".into()),
        Value::Bool(b) => Tag::Byte(i8::from(*b)),
        Value::Number(n) => match n.as_i64() {
            Some(i) if i32::try_from(i).is_ok() => Tag::Int(i as i32),
            Some(i) => Tag::Long(i),
            None => Tag::Double(n.as_f64().unwrap_or(0.0).to_bits()),
        },
        Value::String(s) => Tag::String(s.as_str().into()),
        Value::Array(a) => {
            if a.iter().all(Value::is_string) {
                Tag::List(a.iter().map(tag).collect())
            } else {
                // A list holds one kind: plain strings in a mixed text list become text compounds.
                Tag::List(a.iter().map(|x| if let Value::String(s) = x { tag(&json!({ "text": s })) } else { tag(x) }).collect())
            }
        }
        Value::Object(o) => Tag::compound(o.iter().map(|(k, v)| (k.as_str(), tag(v)))),
    }
}

const WHITE: &str = "#f4f4f5";
const MUTED: &str = "#a1a1aa";
const LAVENDER: &str = "#c4b5fd";
/// Under this, the wait reads "Soon".
const SOON_SECONDS: f64 = 90.0;
const GREEN: &str = "#4ade80";
const AMBER: &str = "#fbbf24";

fn t(text: &str, color: &str) -> Value {
    json!({ "text": text, "color": color })
}

fn tb(text: &str, color: &str) -> Value {
    json!({ "text": text, "color": color, "bold": true })
}

fn line(parts: Vec<Value>) -> Value {
    json!({ "text": "", "extra": parts })
}

/// The queue screen: which server, their place, the wait, their pass (if any), and two buttons. A line appears
/// only when turns are held, to say why, or to greet a player whose place was kept.
pub fn queue_dialog(cfg: &QueueConfig, display: &str, s: &Standing, came_back: bool) -> Tag {
    let tier = cfg.tiers.get(s.tier);
    let default_tier = cfg.tiers.iter().rposition(|t| t.groups.is_empty() && t.permission.is_empty() && !t.bypass);
    let mut lines = vec![
        t(&format!("{display} queue\n"), MUTED),
        tb(&format!("#{}", thousands(s.position)), WHITE),
        t("\n\nEstimated wait  ", MUTED),
        tb(&eta_text(s.eta_seconds), if s.eta_seconds < SOON_SECONDS { GREEN } else { LAVENDER }),
    ];
    if let Some(tr) = tier
        && Some(s.tier) != default_tier
    {
        let name = if tr.display.is_empty() { tr.name.as_str() } else { tr.display.as_str() };
        let color = if tr.color.is_empty() { AMBER } else { tr.color.as_str() };
        lines.push(t("\n\n", WHITE));
        lines.push(tb(&format!("◆ {name}"), color));
    }
    // A staff pause says nothing to the player (owner 11-08): the wait just reads longer.
    if let Some(why) = s.held.as_ref().filter(|w| !w.starts_with("paused")) {
        let mut c = why.chars();
        let why = c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default();
        lines.push(t(&format!("\n\n{why}."), AMBER));
    } else if came_back {
        lines.push(t("\n\nWelcome back: you kept your place.", GREEN));
    }
    let mut actions = Vec::new();
    // The best pass there is (the first tier that isn't a bypass) has nothing left to buy.
    let best = cfg.tiers.iter().position(|t| !t.bypass);
    if !cfg.store_url.is_empty() && best != Some(s.tier) {
        actions.push(json!({ "label": tb("Get Priority", AMBER), "tooltip": t("Skip ahead with a priority pass", WHITE), "width": 200, "action": { "type": "open_url", "url": cfg.store_url } }));
    }
    actions.push(json!({
        "label": t("Leave Queue", "#d4d4d8"),
        "tooltip": t("Back to the hub (your place is kept for a few minutes)", WHITE),
        "width": 200,
        "action": { "type": "custom", "id": "nyxora:queue/leave" }
    }));
    tag(&json!({
        "type": "minecraft:multi_action",
        "title": t("", WHITE),
        "external_title": t("Queue", WHITE),
        "body": [{ "type": "minecraft:plain_message", "contents": line(lines), "width": 260 }],
        "actions": actions,
        "columns": 1,
        "can_close_with_escape": false,
        "pause": false,
        "after_action": "none"
    }))
}

/// 1,234
pub fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "Soon", "2 minutes", "45 minutes", "1 hour", "1 hour 15 minutes", "3 hours" (owner 11-08: no "~19 min").
pub fn eta_text(seconds: f64) -> String {
    let s = seconds.max(0.0);
    if s < SOON_SECONDS {
        return "Soon".into();
    }
    let m = ((s / 60.0).round() as u64).max(2);
    if m < 60 {
        return format!("{m} minutes");
    }
    // Past an hour, to the nearest 5 minutes.
    let m = (m + 2) / 5 * 5;
    let (h, m) = (m / 60, m % 60);
    let hours = if h == 1 { "1 hour".to_owned() } else { format!("{h} hours") };
    if m == 0 { hours } else { format!("{hours} {m} minutes") }
}

/// The server list's hover lines while people wait: one line for each queued server with a line.
pub fn status_sample(lines: &[(String, usize)]) -> Vec<Value> {
    lines
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|(server, n)| json!({ "name": format!("§d{server} queue §7· §f{} waiting", thousands(*n)), "id": "00000000-0000-0000-0000-000000000000" }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_becomes_nbt() {
        let v = json!({ "text": "", "bold": true, "extra": ["a", { "text": "b" }], "width": 300 });
        let Tag::Compound(c) = tag(&v) else { panic!() };
        assert_eq!(c.get(&"bold".into()), Some(&Tag::Byte(1)));
        assert_eq!(c.get(&"width".into()), Some(&Tag::Int(300)));
        let Some(Tag::List(extra)) = c.get(&"extra".into()) else { panic!() };
        assert!(extra.iter().all(|x| matches!(x, Tag::Compound(_))), "mixed lists become compounds");
    }

    #[test]
    fn the_queue_screen_has_its_buttons() {
        let mut cfg = QueueConfig { store_url: "https://store.example".into(), ..QueueConfig::default() };
        cfg.tiers.insert(0, crate::queue::Tier { name: "plus".into(), weight: 6.0, permission: "q.plus".into(), ..Default::default() });
        let s = Standing { server: "smp".into(), position: 120, total: 1450, eta_seconds: 200.0, tier: 1, held: None };
        let count = |d: Tag| match d {
            Tag::Compound(c) => match c.get(&"actions".into()) {
                Some(Tag::List(a)) => a.len(),
                _ => 0,
            },
            _ => 0,
        };
        assert_eq!(count(queue_dialog(&cfg, "Survival", &s, false)), 2, "Get Priority and Leave Queue");
        assert_eq!(count(queue_dialog(&cfg, "Survival", &Standing { tier: 0, ..s.clone() }, false)), 1, "the best pass has nothing to buy");
    }

    #[test]
    fn numbers_and_times() {
        assert_eq!(thousands(1234567), "1,234,567");
        assert_eq!(thousands(12), "12");
        assert_eq!(eta_text(30.0), "Soon");
        assert_eq!(eta_text(80.0), "Soon");
        assert_eq!(eta_text(100.0), "2 minutes");
        assert_eq!(eta_text(300.0), "5 minutes");
        assert_eq!(eta_text(1140.0), "19 minutes");
        assert_eq!(eta_text(3600.0), "1 hour");
        assert_eq!(eta_text(4500.0), "1 hour 15 minutes");
        assert_eq!(eta_text(7300.0), "2 hours");
        assert_eq!(status_sample(&[("Survival".into(), 0)]).len(), 0);
        assert_eq!(status_sample(&[("Survival".into(), 1200)])[0]["name"], "§dSurvival queue §7· §f1,200 waiting");
    }
}
