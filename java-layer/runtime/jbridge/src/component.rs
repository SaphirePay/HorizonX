//! Adventure: `Component`, its builders, styles, events, serializers and translations. A component is
//! the game's own text JSON, as in the SDK, so everything sends as it is.
use crate::{held, resource_names, resource_text, wrap};
use horizon_bukkit::Component;
use jrt::{R, V};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

/// `Component.text()` and the other builders, and `Style`.
pub struct BuilderH(pub Mutex<Value>);
pub struct EventH(pub &'static str, pub Value);
pub struct JoinH(pub Option<Value>);
pub struct SerializerH(pub char);
/// `LegacyComponentSerializer.builder()`, until `build()`.
pub struct SerializerBuilderH(pub std::sync::Mutex<char>);
pub struct TranslatorH;

fn builder(value: Value) -> V {
    jrt::native(BuilderH(Mutex::new(value)))
}

fn comp(value: Value) -> V {
    wrap(Component(value))
}

/// Anything Java can hand over as a `ComponentLike`.
pub fn json_of(value: &V) -> Option<Value> {
    if let Some(found) = held::<Component>(value) {
        return Some(found.0.clone());
    }
    if let Some(found) = value.native::<BuilderH>() {
        return Some(jrt::lock(&found.0).clone());
    }
    match value {
        V::Null => None,
        V::Obj(_) => jrt::call(value, "asComponent", &[]).ok().and_then(|c| json_of(&c)),
        V::Func(_) => jrt::call(value, "$invoke", &[]).ok().and_then(|c| json_of(&c)),
        other => Some(json!({ "text": other.text() })),
    }
}

/// Adventure's `ComponentLike`: a component, a builder, or a plugin object that answers `asComponent`.
pub fn component_like(value: &V) -> bool {
    held::<Component>(value).is_some() || value.native::<BuilderH>().is_some() || (matches!(value, V::Obj(_)) && jrt::instance_of(value, "ComponentLike"))
}

/// The component a `ComponentLike` stands for (a builder is built).
pub fn component_of(value: &V) -> Option<Component> {
    if !component_like(value) {
        return None;
    }
    json_of(value).map(Component)
}

/// A message argument of an event: Java's `String` overload where the SDK takes a `Component`.
pub fn message_arg(value: &V) -> Option<Component> {
    match value {
        V::Str(text) => Some(Component(legacy_text(text, '\u{a7}'))),
        other => json_of(other).map(Component),
    }
}

fn text_of(value: &V) -> String {
    value.text()
}

fn set_color(target: &mut Value, color: &V) {
    if let Some(map) = target.as_object_mut() {
        if color.is_null() {
            map.remove("color");
        } else {
            map.insert("color".to_owned(), Value::String(color.text()));
        }
    }
}

fn set_decoration(target: &mut Value, name: &V, on: bool) {
    target[name.text().to_lowercase()] = Value::Bool(on);
}

fn truth_of(state: &V) -> bool {
    state.truth() || state.text().eq_ignore_ascii_case("true")
}

/// What components and builders share. `None`: not one of these.
fn style_op(target: &mut Value, name: &str, a: &[V]) -> Option<()> {
    match (name, a.len()) {
        ("color" | "colorIfAbsent", 1) => set_color(target, &a[0]),
        ("decorate", _) if !a.is_empty() => {
            for decoration in a {
                set_decoration(target, decoration, true);
            }
        }
        ("decoration" | "decorationIfAbsent", 2) => set_decoration(target, &a[0], truth_of(&a[1])),
        ("append", 1) => {
            let Some(child) = json_of(&a[0]) else { return Some(()) };
            let extra = target.as_object_mut()?.entry("extra").or_insert_with(|| json!([]));
            extra.as_array_mut()?.push(child);
        }
        ("appendNewline" | "appendSpace", 0) => {
            let piece = if name == "appendNewline" { "\n" } else { " " };
            let extra = target.as_object_mut()?.entry("extra").or_insert_with(|| json!([]));
            extra.as_array_mut()?.push(json!({ "text": piece }));
        }
        ("clickEvent" | "hoverEvent", 1) => {
            if let Some(event) = a[0].native::<EventH>() {
                target[event.0] = event.1.clone();
            } else if name == "hoverEvent" {
                if let Some(text) = json_of(&a[0]) {
                    target["hover_event"] = json!({ "action": "show_text", "value": text });
                }
            }
        }
        ("insertion", 1) => target["insertion"] = Value::String(a[0].text()),
        ("style" | "mergeStyle", 1) => {
            if let (Some(style), Some(map)) = (json_of(&a[0]), target.as_object_mut()) {
                for (key, value) in style.as_object()? {
                    if key != "text" {
                        map.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        ("content", 1) => target["text"] = Value::String(a[0].text()),
        ("key", 1) => {
            let map = target.as_object_mut()?;
            map.remove("text");
            map.insert("translate".to_owned(), Value::String(a[0].text()));
        }
        ("fallback", 1) => target["fallback"] = Value::String(a[0].text()),
        ("args" | "arguments", _) => {
            let list: Vec<V> = match a {
                [one] if matches!(one, V::Arr(_) | V::List(_)) => one.items().unwrap_or_default(),
                _ => a.to_vec(),
            };
            target["with"] = Value::Array(list.iter().filter_map(json_of).collect());
        }
        _ => return None,
    }
    Some(())
}

fn read_op(value: &Value, name: &str, a: &[V]) -> Option<V> {
    Some(match (name, a.len()) {
        ("content", 0) => jrt::s(value["text"].as_str().unwrap_or("")),
        ("key", 0) => jrt::s(value["translate"].as_str().unwrap_or("")),
        ("children", 0) => jrt::list(value["extra"].as_array().into_iter().flatten().cloned().map(comp).collect()),
        ("color", 0) => value["color"].as_str().map_or(V::Null, jrt::s),
        ("style", 0) => {
            let style: Map<String, Value> = value.as_object()?.iter().filter(|(k, _)| !matches!(k.as_str(), "text" | "extra" | "translate" | "with")).map(|(k, v)| (k.clone(), v.clone())).collect();
            builder(Value::Object(style))
        }
        ("isEmpty", 0) => V::Bool(value.as_object().is_some_and(|m| m.keys().all(|k| matches!(k.as_str(), "text" | "extra")))),
        ("hasDecoration", 1) | ("decoration", 1) => V::Bool(value[a[0].text().to_lowercase()].as_bool() == Some(true)),
        ("hasStyling", 0) => V::Bool(value.as_object().is_some_and(|m| m.keys().any(|k| k != "text" && k != "extra"))),
        ("clickEvent" | "hoverEvent" | "insertion", 0) => V::Null,
        _ => return None,
    })
}

fn translatable(a: &[V]) -> V {
    let mut value = json!({ "translate": a[0].text() });
    let mut args = Vec::new();
    for extra in &a[1..] {
        match extra {
            V::List(_) | V::Arr(_) => args.extend(extra.items().unwrap_or_default().iter().filter_map(json_of)),
            V::Str(style) if !style.contains(' ') && style.chars().all(|c| c.is_ascii_lowercase() || c == '_' || c == '#' || c.is_ascii_digit()) => set_color(&mut value, extra),
            other => args.extend(json_of(other)),
        }
    }
    if !args.is_empty() {
        value["with"] = Value::Array(args);
    }
    comp(value)
}

fn click(action: &str, field: &str, a: &[V]) -> V {
    jrt::native(EventH("click_event", json!({ "action": action, field: a[0].text() })))
}

fn legacy_text(text: &str, marker: char) -> Value {
    horizon_bukkit::adventure::legacy(&text.replace(marker, "\u{a7}")).0
}

pub fn static_method(class: &str, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match (class, name, a.len()) {
        ("Component" | "TextComponent", "text", 0) => builder(json!({ "text": "" })),
        ("Component", "text", 1) if matches!(a[0], V::Func(_)) => {
            let made = builder(json!({ "text": "" }));
            if let Err(error) = jrt::call(&a[0], "$invoke", std::slice::from_ref(&made)) {
                return Some(Err(error));
            }
            comp(json_of(&made)?)
        }
        ("Component", "text", _) => {
            let mut value = json!({ "text": text_of(&a[0]) });
            for style in &a[1..] {
                match style {
                    V::Str(text) if text.len() > 1 && matches!(&**text, "bold" | "BOLD" | "italic" | "ITALIC" | "underlined" | "UNDERLINED" | "strikethrough" | "STRIKETHROUGH" | "obfuscated" | "OBFUSCATED") => set_decoration(&mut value, style, true),
                    V::Str(_) => set_color(&mut value, style),
                    V::Null => {}
                    other => {
                        style_op(&mut value, "style", std::slice::from_ref(other));
                    }
                }
            }
            comp(value)
        }
        ("Component", "empty", 0) => comp(json!({ "text": "" })),
        ("Component", "newline", 0) => comp(json!({ "text": "\n" })),
        ("Component", "space", 0) => comp(json!({ "text": " " })),
        ("Component", "translatable", 0) => builder(json!({ "translate": "" })),
        ("Component", "translatable", _) => translatable(a),
        ("Component", "join", 2) => {
            let separator = a[0].native::<JoinH>().and_then(|j| j.0.clone()).or_else(|| json_of(&a[0]));
            let mut parts = Vec::new();
            let items: Vec<V> = a[1].items().ok()?;
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    parts.extend(separator.clone());
                }
                parts.extend(json_of(item));
            }
            comp(json!({ "text": "", "extra": parts }))
        }
        ("JoinConfiguration", "separator", 1) => jrt::native(JoinH(json_of(&a[0]))),
        ("JoinConfiguration", "newlines", 0) => jrt::native(JoinH(Some(json!({ "text": "\n" })))),
        ("JoinConfiguration", "noSeparators", 0) => jrt::native(JoinH(None)),
        ("JoinConfiguration", "spaces", 0) => jrt::native(JoinH(Some(json!({ "text": " " })))),
        ("TextColor", "color", 1) => jrt::s(&format!("#{:06x}", a[0].int() & 0xff_ffff)),
        ("TextColor", "color", 3) => jrt::s(&format!("#{:02x}{:02x}{:02x}", a[0].int(), a[1].int(), a[2].int())),
        ("TextColor", "fromHexString", 1) => a[0].clone(),
        ("HoverEvent", "showText", 1) => jrt::native(EventH("hover_event", json!({ "action": "show_text", "value": json_of(&a[0])? }))),
        // `ClickEvent.clickEvent(ClickEvent.Action.RUN_COMMAND, "/x")`: by its action's name.
        ("ClickEvent", "clickEvent", 2) => {
            let action = a[0].text().rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
            let key = match action.as_str() {
                "open_url" => "url",
                "copy_to_clipboard" => "value",
                "change_page" => "page",
                _ => "command",
            };
            click(&action, key, &a[1..])
        }
        ("ClickEvent", "runCommand", 1) => click("run_command", "command", a),
        ("ClickEvent", "suggestCommand", 1) => click("suggest_command", "command", a),
        ("ClickEvent", "openUrl", 1) => click("open_url", "url", a),
        ("ClickEvent", "copyToClipboard", 1) => click("copy_to_clipboard", "value", a),
        ("Style", "empty" | "style", 0) => builder(json!({ "text": "" })),
        ("Style", "style", _) => {
            let mut value = json!({ "text": "" });
            for style in a {
                match style {
                    V::Str(text) if matches!(&**text, "bold" | "italic" | "underlined" | "strikethrough" | "obfuscated") => set_decoration(&mut value, style, true),
                    V::Str(_) => set_color(&mut value, style),
                    _ => {}
                }
            }
            builder(value)
        }
        ("LegacyComponentSerializer", "legacySection", 0) => jrt::native(SerializerH('\u{a7}')),
        ("LegacyComponentSerializer", "legacyAmpersand", 0) => jrt::native(SerializerH('&')),
        ("LegacyComponentSerializer", "legacy", 1) => jrt::native(SerializerH(a[0].text().chars().next().unwrap_or('&'))),
        // `builder().character('&').hexColors()...build()`: the character is what tells serializers apart here.
        ("LegacyComponentSerializer", "builder", 0) => jrt::native(SerializerBuilderH(std::sync::Mutex::new('\u{a7}'))),
        ("PlainTextComponentSerializer", "plainText", 0) => jrt::native(SerializerH('\0')),
        ("GsonComponentSerializer", "gson", 0) => jrt::native(SerializerH('{')),
        ("GlobalTranslator", "render", 2) => comp(render(&json_of(&a[0])?)),
        ("GlobalTranslator", "translator" | "get", 0) => jrt::native(TranslatorH),
        ("Translator", "parseLocale", 1) => a[0].clone(),
        _ => return None,
    }))
}

fn serializer_method(serializer: &SerializerH, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match (serializer.0, name) {
        ('\0', "serialize") => {
            let rendered = render(&json_of(&a[0])?);
            jrt::s(&Component(rendered).plain())
        }
        ('\0', "deserialize") => comp(json!({ "text": a[0].text() })),
        ('{', "serialize") => jrt::s(&json_of(&a[0])?.to_string()),
        ('{', "deserialize") => comp(serde_json::from_str(&a[0].text()).unwrap_or_else(|_| json!({ "text": "" }))),
        (marker, "serialize") => {
            let text = Component(render(&json_of(&a[0])?)).to_legacy();
            jrt::s(&if marker == '&' { text.replace('\u{a7}', "&") } else { text })
        }
        (marker, "deserialize") => comp(legacy_text(&a[0].text(), marker)),
        _ => return None,
    }))
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if let Some(found) = this.native::<BuilderH>() {
        let mut value = jrt::lock(&found.0);
        if style_op(&mut value, name, a).is_some() {
            return Some(Ok(this.clone()));
        }
        return Some(Ok(match name {
            "build" | "asComponent" => comp(value.clone()),
            "apply" | "applicableApply" => {
                drop(value);
                return Some(jrt::call(&a[0], "$invoke", std::slice::from_ref(this)).map(|_| this.clone()));
            }
            "toString" => jrt::s(&value.to_string()),
            _ => return read_op(&value, name, a).map(Ok),
        }));
    }
    if let Some(found) = this.native::<SerializerH>() {
        return serializer_method(found, name, a);
    }
    if let Some(found) = this.native::<SerializerBuilderH>() {
        return Some(Ok(match name {
            "character" => {
                *jrt::lock(&found.0) = a[0].text().chars().next().unwrap_or('&');
                this.clone()
            }
            "build" => jrt::native(SerializerH(*jrt::lock(&found.0))),
            // Hex colours, URL extraction, flattening: the legacy text reader already takes `&#rrggbb`.
            _ => this.clone(),
        }));
    }
    if this.native::<TranslatorH>().is_some() {
        return Some(Ok(V::Bool(true)));
    }
    if this.native::<Mutex<Component>>().is_some() {
        let mut value = held::<Component>(this)?.0.clone();
        if style_op(&mut value, name, a).is_some() {
            return Some(Ok(comp(value)));
        }
        return Some(Ok(match name {
            "asComponent" | "compact" => this.clone(),
            "toBuilder" => builder(value),
            "toString" => jrt::s(&value.to_string()),
            "equals" => V::Bool(json_of(&a[0]).is_some_and(|o| o == value)),
            "hashCode" => V::Int(value.to_string().bytes().fold(0, |h: i32, b| h.wrapping_mul(31).wrapping_add(i32::from(b)))),
            _ => return read_op(&value, name, a).map(Ok),
        }));
    }
    None
}

// ---- Translations: the jar's `.properties`, as `MessageFormat` reads them ----

fn table() -> &'static BTreeMap<String, String> {
    static TABLE: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut out = BTreeMap::new();
        for name in resource_names().into_iter().filter(|n| n.ends_with(".properties")) {
            if let Some(text) = resource_text(name) {
                parse_properties(&text, &mut out);
            }
        }
        out
    })
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('u') => {
                let code: String = chars.by_ref().take(4).collect();
                out.extend(u32::from_str_radix(&code, 16).ok().and_then(char::from_u32));
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

fn parse_properties(text: &str, out: &mut BTreeMap<String, String>) {
    let mut lines = text.lines();
    while let Some(first) = lines.next() {
        let mut line = first.trim_start().to_owned();
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            continue;
        }
        while line.ends_with('\\') {
            line.pop();
            line.push_str(lines.next().unwrap_or("").trim_start());
        }
        if let Some((key, value)) = line.split_once('=') {
            out.insert(key.trim().to_owned(), unescape(value.trim_start()));
        }
    }
}

/// `MessageFormat`: `{0}` is an argument, `''` a quote, and `'...'` is literal.
fn format_message(template: &str, args: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut chars = template.chars().peekable();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match c {
            '\'' if chars.peek() == Some(&'\'') => {
                chars.next();
                text.push('\'');
            }
            '\'' => quoted = !quoted,
            '{' if !quoted => {
                let mut index = String::new();
                for d in chars.by_ref() {
                    if d == '}' {
                        break;
                    }
                    index.push(d);
                }
                if !text.is_empty() {
                    out.push(json!({ "text": std::mem::take(&mut text) }));
                }
                if let Some(arg) = index.trim().parse::<usize>().ok().and_then(|i| args.get(i)) {
                    out.push(arg.clone());
                }
            }
            other => text.push(other),
        }
    }
    if !text.is_empty() {
        out.push(json!({ "text": text }));
    }
    out
}

pub fn render(value: &Value) -> Value {
    let Some(map) = value.as_object() else { return value.clone() };
    let mut out: Map<String, Value> = map.clone();
    let mut extra: Vec<Value> = map.get("extra").and_then(Value::as_array).into_iter().flatten().map(render).collect();
    if let Some(template) = map.get("translate").and_then(Value::as_str).and_then(|key| table().get(key)) {
        let args: Vec<Value> = map.get("with").and_then(Value::as_array).into_iter().flatten().map(render).collect();
        let mut pieces = format_message(template, &args);
        pieces.append(&mut extra);
        extra = pieces;
        out.remove("translate");
        out.remove("with");
        out.insert("text".to_owned(), json!(""));
    }
    if !extra.is_empty() {
        out.insert("extra".to_owned(), Value::Array(extra));
    }
    if let Some(hover) = out.get_mut("hover_event") {
        if let Some(text) = hover.get("value") {
            let text = render(text);
            hover["value"] = text;
        }
    }
    Value::Object(out)
}
