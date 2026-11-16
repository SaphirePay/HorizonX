//! BungeeCord's chat API (`net.md_5.bungee.api.chat`), which Spigot plugins send with
//! `player.spigot().sendMessage(...)`: `TextComponent`, `ComponentBuilder`, `ClickEvent`, `HoverEvent`,
//! `Text`. A BaseComponent is the game's text JSON held as the SDK's `Component`, as Adventure's are
//! (crate::component), so both kinds send, compare and serialize the same way. Constants such as
//! `ClickEvent.Action.RUN_COMMAND` and `ChatMessageType.ACTION_BAR` arrive as their names, and
//! `ChatColor.RED` as its legacy code (`§c`).
use crate::component::{EventH, json_of};
use crate::{held, wrap};
use horizon_bukkit::{CommandSender, Component, Player};
use jrt::{R, V};
use serde_json::{Map, Value, json};
use std::sync::Mutex;

/// `new ComponentBuilder(...)`: the parts so far; formatting and events go on the last one.
pub struct BuilderB(pub Mutex<Vec<Value>>);
/// `new Text(...)`: a hover's text content.
pub struct HoverText(pub Value);

const COLORS: [(&str, char); 16] = [
    ("black", '0'), ("dark_blue", '1'), ("dark_green", '2'), ("dark_aqua", '3'), ("dark_red", '4'), ("dark_purple", '5'), ("gold", '6'), ("gray", '7'),
    ("dark_gray", '8'), ("blue", '9'), ("green", 'a'), ("aqua", 'b'), ("red", 'c'), ("light_purple", 'd'), ("yellow", 'e'), ("white", 'f'),
];
const DECORATIONS: [(&str, char); 5] = [("obfuscated", 'k'), ("bold", 'l'), ("strikethrough", 'm'), ("underlined", 'n'), ("italic", 'o')];
const FORMAT_KEYS: [&str; 8] = ["color", "bold", "italic", "underlined", "strikethrough", "obfuscated", "font", "insertion"];
const EVENT_KEYS: [&str; 2] = ["click_event", "hover_event"];

fn comp(value: Value) -> V {
    wrap(Component(value))
}

fn legacy(text: &str) -> Value {
    horizon_bukkit::adventure::legacy(text).0
}

/// A ChatColor (`§c`, `§x§f§f§8§8§0§0`, `#ff8800` or a name) as the JSON's colour; None for a format code.
fn color_of(color: &V) -> Option<String> {
    let text = color.text();
    let codes: Vec<char> = text.chars().filter(|c| *c != '\u{a7}').collect();
    if text.starts_with('\u{a7}') {
        if codes.first() == Some(&'x') && codes.len() >= 7 {
            return Some(format!("#{}", codes[1..7].iter().collect::<String>().to_ascii_lowercase()));
        }
        let code = codes.first()?.to_ascii_lowercase();
        return COLORS.iter().find(|(_, c)| *c == code).map(|(name, _)| (*name).to_owned());
    }
    (!text.is_empty()).then(|| text.to_ascii_lowercase())
}

/// What a value stands for as one component: a BaseComponent, an array of them, or text.
fn one(value: &V) -> Value {
    match value {
        V::Arr(_) | V::List(_) => {
            let parts: Vec<Value> = value.items().unwrap_or_default().iter().filter_map(json_of).collect();
            if parts.len() == 1 { parts.into_iter().next().unwrap_or_default() } else { json!({ "text": "", "extra": parts }) }
        }
        V::Str(text) => json!({ "text": text.to_string() }),
        other => json_of(other).unwrap_or_else(|| json!({ "text": "" })),
    }
}

/// Whether a value is a BaseComponent array (`fromLegacyText`, `create()`), to send as one message.
fn component_array(value: &V) -> bool {
    matches!(value, V::Arr(_)) && value.items().is_ok_and(|items| !items.is_empty() && items.iter().all(|item| item.native::<Mutex<Component>>().is_some()))
}

fn event_json(action: &str, value: &V) -> Option<V> {
    let action = action.rsplit('.').next().unwrap_or(action).to_ascii_lowercase();
    let field = match action.as_str() {
        "run_command" | "suggest_command" => "command",
        "open_url" => "url",
        "open_file" => "path",
        "change_page" => "page",
        "copy_to_clipboard" => "value",
        "show_text" => {
            let shown = value.native::<HoverText>().map(|t| t.0.clone()).unwrap_or_else(|| one(value));
            return Some(jrt::native(EventH("hover_event", json!({ "action": "show_text", "value": shown }))));
        }
        "show_item" | "show_entity" => return Some(V::Null),
        _ => return None,
    };
    let given = if field == "page" { json!(value.text().parse::<i64>().unwrap_or(1)) } else { json!(value.text()) };
    Some(jrt::native(EventH("click_event", json!({ "action": action, field: given }))))
}

/// `new TextComponent(...)`, `new ComponentBuilder(...)`, `new ClickEvent(action, value)`,
/// `new HoverEvent(action, contents)`, `new Text(...)`.
pub fn construct(class: &str, a: &[V]) -> Option<R> {
    Some(Ok(match (class, a.len()) {
        ("TextComponent", 0) => comp(json!({ "text": "" })),
        ("TextComponent", 1) if matches!(a[0], V::Str(_) | V::Sb(_)) => comp(json!({ "text": a[0].text() })),
        // A copy of another, or of several (`new TextComponent(BaseComponent...)`).
        ("TextComponent", _) => comp(if a.len() == 1 { one(&a[0]) } else { json!({ "text": "", "extra": a.iter().map(one).collect::<Vec<_>>() }) }),
        ("ComponentBuilder", 0) => jrt::native(BuilderB(Mutex::new(Vec::new()))),
        ("ComponentBuilder", 1) => {
            let first = if matches!(a[0], V::Str(_) | V::Sb(_)) { json!({ "text": a[0].text() }) } else { one(&a[0]) };
            jrt::native(BuilderB(Mutex::new(vec![first])))
        }
        ("ClickEvent" | "HoverEvent", 2) => return event_json(&a[0].text(), &a[1]).map(Ok),
        ("HoverEvent", n) if n > 2 => return event_json(&a[0].text(), &jrt::arr(a[1..].to_vec())).map(Ok),
        ("Text", 1) => jrt::native(HoverText(one(&a[0]))),
        _ => return None,
    }))
}

/// `TextComponent.fromLegacyText(text)` / `fromLegacy(text)`, `TextComponent.toLegacyText(parts...)`.
pub fn static_method(class: &str, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match (class, name) {
        ("TextComponent", "fromLegacyText") if !a.is_empty() => jrt::arr(vec![comp(legacy(&a[0].text()))]),
        ("TextComponent", "fromLegacy") if !a.is_empty() => comp(legacy(&a[0].text())),
        ("TextComponent" | "BaseComponent", "toLegacyText") => jrt::s(&Component(one(&jrt::arr(spread(a)))).to_legacy()),
        ("TextComponent" | "BaseComponent", "toPlainText") => jrt::s(&Component(one(&jrt::arr(spread(a)))).plain()),
        _ => return None,
    }))
}

fn spread(a: &[V]) -> Vec<V> {
    match a {
        [one @ (V::Arr(_) | V::List(_))] => one.items().unwrap_or_default(),
        many => many.to_vec(),
    }
}

/// Formatting from `from` onto `to` where `to` has none of its own, as Bungee's `copyFormatting`.
fn inherit(to: &mut Value, from: &Value, keys: &[&str]) {
    let (Some(to), Some(from)) = (to.as_object_mut(), from.as_object()) else { return };
    for key in keys {
        if !to.contains_key(*key) {
            if let Some(value) = from.get(*key) {
                to.insert((*key).to_owned(), value.clone());
            }
        }
    }
}

fn retention(value: Option<&V>) -> Vec<&'static str> {
    match value.map(|v| v.text()).as_deref().map(|t| t.rsplit('.').next().unwrap_or(t).to_owned()).as_deref() {
        Some("NONE") => Vec::new(),
        Some("FORMATTING") => FORMAT_KEYS.to_vec(),
        Some("EVENTS") => EVENT_KEYS.to_vec(),
        _ => FORMAT_KEYS.iter().chain(EVENT_KEYS.iter()).copied().collect(),
    }
}

/// A style setter on one component's JSON: `color`, `bold(true)`, `event(click)`... True when it was one.
fn style(target: &mut Map<String, Value>, name: &str, a: &[V]) -> bool {
    let flag = |a: &[V]| a.first().map(|v| if v.is_null() { Value::Null } else { Value::Bool(v.truth()) });
    let key = name.strip_prefix("set").map_or_else(|| name.to_owned(), |rest| rest[..1].to_ascii_lowercase() + &rest[1..]);
    match key.as_str() {
        "color" => match a.first().filter(|c| !c.is_null()).and_then(color_of) {
            Some(color) => {
                target.insert("color".to_owned(), Value::String(color));
            }
            None => {
                // `ChatColor.BOLD` given as a colour: Bungee turns that format on.
                let code = a.first().map(|c| c.text().chars().filter(|c| *c != '\u{a7}').collect::<String>()).unwrap_or_default();
                match DECORATIONS.iter().find(|(_, c)| code.starts_with(*c)) {
                    Some((decoration, _)) => {
                        target.insert((*decoration).to_owned(), Value::Bool(true));
                    }
                    None => {
                        target.remove("color");
                    }
                }
            }
        },
        "bold" | "italic" | "underlined" | "strikethrough" | "obfuscated" => match flag(a) {
            Some(Value::Null) | None => {
                target.remove(&key);
            }
            Some(on) => {
                target.insert(key, on);
            }
        },
        "font" | "insertion" => {
            match a.first().filter(|v| !v.is_null()) {
                Some(v) => target.insert(key, Value::String(v.text())),
                None => target.remove(&key),
            };
        }
        "clickEvent" | "hoverEvent" | "event" => {
            match a.first().and_then(|v| v.native::<EventH>()) {
                Some(event) => target.insert(event.0.to_owned(), event.1.clone()),
                None if key != "event" => target.remove(if key == "clickEvent" { "click_event" } else { "hover_event" }),
                None => None,
            };
        }
        _ => return false,
    }
    true
}

/// `sendMessage(BaseComponent[])`, `sendMessage(ChatMessageType, BaseComponent...)`: one message, as Bungee sends.
fn send(target: &V, a: &[V]) -> Option<R> {
    let (kind, parts) = match a {
        [V::Str(kind), rest @ ..] if matches!(&**kind, "CHAT" | "SYSTEM" | "ACTION_BAR") && !rest.is_empty() => (kind.to_string(), rest),
        [single] if component_array(single) => ("CHAT".to_owned(), a),
        many if many.len() > 1 && many.iter().all(|v| v.native::<Mutex<Component>>().is_some()) => ("CHAT".to_owned(), a),
        _ => return None,
    };
    let text = Component(if parts.len() == 1 { one(&parts[0]) } else { json!({ "text": "", "extra": parts.iter().map(one).collect::<Vec<_>>() }) });
    if let Some(player) = held::<Player>(target).map(|p| p.clone()) {
        if kind == "ACTION_BAR" { player.send_action_bar(&text) } else { player.send_component(&text) }
        return Some(Ok(V::Null));
    }
    if let Some(sender) = held::<CommandSender>(target).map(|s| s.clone()) {
        sender.send_component(&text);
        return Some(Ok(V::Null));
    }
    None
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if name == "sendMessage" {
        if let Some(found) = send(this, a) {
            return Some(found);
        }
    }
    if let Some(builder) = this.native::<BuilderB>() {
        let mut parts = jrt::lock(&builder.0);
        return Some(Ok(match name {
            "append" if !a.is_empty() => {
                let keep = retention(a.get(1));
                let previous = parts.last().cloned().unwrap_or_else(|| json!({}));
                let added: Vec<Value> = match &a[0] {
                    V::Str(_) | V::Sb(_) => vec![json!({ "text": a[0].text() })],
                    many @ (V::Arr(_) | V::List(_)) => many.items().unwrap_or_default().iter().filter_map(json_of).collect(),
                    other => json_of(other).into_iter().collect(),
                };
                for mut part in added {
                    inherit(&mut part, &previous, &keep);
                    parts.push(part);
                }
                this.clone()
            }
            "appendLegacy" if !a.is_empty() => {
                parts.push(legacy(&a[0].text()));
                this.clone()
            }
            "reset" | "retain" => {
                let keep = if name == "reset" { Vec::new() } else { retention(a.first()) };
                if let Some(Value::Object(last)) = parts.last_mut() {
                    last.retain(|key, _| key == "text" || key == "extra" || key == "translate" || key == "with" || keep.contains(&key.as_str()));
                }
                this.clone()
            }
            "create" => jrt::arr(parts.iter().cloned().map(comp).collect()),
            "build" => comp(if parts.len() == 1 { parts[0].clone() } else { json!({ "text": "", "extra": parts.clone() }) }),
            "getParts" => jrt::list(parts.iter().cloned().map(comp).collect()),
            "getCursor" => V::Int(parts.len() as i32 - 1),
            _ => {
                if parts.is_empty() {
                    parts.push(json!({ "text": "" }));
                }
                let Some(Value::Object(last)) = parts.last_mut() else { return None };
                if !style(last, name, a) {
                    return None;
                }
                this.clone()
            }
        }));
    }
    // BungeeCord's setters and getters on a component; Adventure's own names stay with crate::component.
    if this.native::<Mutex<Component>>().is_some() {
        let bungee = matches!(name, "setText" | "getText" | "addExtra" | "setExtra" | "getExtra" | "toLegacyText" | "toPlainText" | "duplicate" | "getColor" | "isBold" | "isItalic" | "isUnderlined" | "isStrikethrough" | "isObfuscated" | "getClickEvent" | "getHoverEvent")
            || (name.starts_with("set") && matches!(name, "setColor" | "setBold" | "setItalic" | "setUnderlined" | "setStrikethrough" | "setObfuscated" | "setClickEvent" | "setHoverEvent" | "setFont" | "setInsertion"));
        if !bungee {
            return None;
        }
        let mut held_ = held::<Component>(this)?;
        if !held_.0.is_object() {
            held_.0 = json!({ "text": held_.0.as_str().unwrap_or_default() });
        }
        let whole = held_.0.clone();
        match name {
            "toLegacyText" => return Some(Ok(jrt::s(&Component(whole).to_legacy()))),
            "toPlainText" => return Some(Ok(jrt::s(&Component(whole).plain()))),
            "duplicate" => return Some(Ok(comp(whole))),
            _ => {}
        }
        let map = held_.0.as_object_mut()?;
        if style(map, name, a) {
            return Some(Ok(V::Null));
        }
        return Some(Ok(match name {
            "setText" => {
                map.insert("text".to_owned(), Value::String(a.first().map(|v| v.text()).unwrap_or_default()));
                V::Null
            }
            "getText" => jrt::s(map.get("text").and_then(Value::as_str).unwrap_or_default()),
            "addExtra" if !a.is_empty() => {
                let part = if matches!(a[0], V::Str(_) | V::Sb(_)) { json!({ "text": a[0].text() }) } else { one(&a[0]) };
                match map.get_mut("extra").and_then(Value::as_array_mut) {
                    Some(extra) => extra.push(part),
                    None => {
                        map.insert("extra".to_owned(), Value::Array(vec![part]));
                    }
                }
                V::Null
            }
            "setExtra" => {
                let parts: Vec<Value> = a.first().and_then(|v| v.items().ok()).unwrap_or_default().iter().map(one).collect();
                map.insert("extra".to_owned(), Value::Array(parts));
                V::Null
            }
            "getExtra" => match map.get("extra").and_then(Value::as_array) {
                Some(extra) => jrt::list(extra.iter().cloned().map(comp).collect()),
                None => V::Null,
            },
            "getColor" => match map.get("color").and_then(Value::as_str) {
                Some(color) => match COLORS.iter().find(|(name, _)| *name == color) {
                    Some((_, code)) => jrt::s(&format!("\u{a7}{code}")),
                    None => jrt::s(color),
                },
                None => jrt::s("\u{a7}f"),
            },
            "isBold" | "isItalic" | "isUnderlined" | "isStrikethrough" | "isObfuscated" => {
                let key = name[2..].to_ascii_lowercase();
                V::Bool(map.get(&key).and_then(Value::as_bool).unwrap_or(false))
            }
            "getClickEvent" | "getHoverEvent" => {
                let key = if name == "getClickEvent" { "click_event" } else { "hover_event" };
                match map.get(key) {
                    Some(event) => jrt::native(EventH(key, event.clone())),
                    None => V::Null,
                }
            }
            _ => return None,
        }));
    }
    None
}
