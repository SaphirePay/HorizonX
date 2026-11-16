//! MiniMessage, all of what plugins' configs use: colours (`<red>`, `<#ff00aa>`, `<color:x>`), decorations and
//! their negations, `<reset>`, `<newline>`/`<br>`, `<gradient:a:b:..[:phase]>`, `<rainbow[:!][phase]>`,
//! `<transition:a:b:phase>`, `<hover:show_text:'..'>`, `<click:action:'..'>`, `<insert:..>`, `<font:..>`,
//! `<key:..>`, `<lang:..>`; optionally legacy `&`/`§` codes and `&#rrggbb` mixed in (LPC, TAB and AnnouncerPlus
//! configs do), and your own tags (`<online_players>`) through a resolver.
//!
//! Output is the game's text JSON (`click_event`/`hover_event` as the current protocol names them), the same
//! shape `horizon_bukkit::adventure::Component` holds; [`legacy`] gives a `§` string for the calls that take one.

use serde_json::{Map, Value, json};

const NAMED: [(&str, u32); 16] = [
    ("black", 0x000000), ("dark_blue", 0x0000AA), ("dark_green", 0x00AA00), ("dark_aqua", 0x00AAAA), ("dark_red", 0xAA0000), ("dark_purple", 0xAA00AA),
    ("gold", 0xFFAA00), ("gray", 0xAAAAAA), ("dark_gray", 0x555555), ("blue", 0x5555FF), ("green", 0x55FF55), ("aqua", 0x55FFFF), ("red", 0xFF5555),
    ("light_purple", 0xFF55FF), ("yellow", 0xFFFF55), ("white", 0xFFFFFF),
];
const LEGACY_ORDER: &str = "0123456789abcdef";

/// How to read the input.
#[derive(Clone, Copy, Default)]
pub struct Opts {
    /// `&c`, `§c`, `&#rrggbb`, `&x&r&r..` read as colours too.
    pub legacy: bool,
}

/// A colour name or `#rrggbb` to its rgb.
pub fn rgb_of(name: &str) -> Option<u32> {
    let n = name.trim().to_ascii_lowercase();
    let n = match n.as_str() {
        "grey" => "gray",
        "dark_grey" => "dark_gray",
        other => other,
    };
    if let Some(hex) = n.strip_prefix('#').or_else(|| n.strip_prefix("&#")) {
        return (hex.len() == 6).then(|| u32::from_str_radix(hex, 16).ok()).flatten();
    }
    NAMED.iter().find(|(k, _)| *k == n).map(|(_, v)| *v)
}

fn color_value(rgb: u32) -> Value {
    // Named colours stay named (old clients, `to_legacy` maps them to their code).
    match NAMED.iter().find(|(_, v)| *v == rgb) {
        Some((name, _)) => json!(name),
        None => json!(format!("#{rgb:06X}")),
    }
}

fn decoration(tag: &str) -> Option<&'static str> {
    Some(match tag {
        "bold" | "b" => "bold",
        "italic" | "i" | "em" => "italic",
        "underlined" | "u" | "underline" => "underlined",
        "strikethrough" | "st" => "strikethrough",
        "obfuscated" | "obf" => "obfuscated",
        _ => return None,
    })
}

#[derive(Clone, Debug)]
enum Tag {
    Style(String, Value),
    Gradient(Vec<u32>, f32),
    Rainbow(bool, f32),
    /// A legacy code's frame: closed by the next legacy colour or `&r`.
    Legacy(String, Value),
}

#[derive(Clone, Debug)]
enum Node {
    Text(String),
    Comp(Value),
    El(Tag, Vec<Node>),
}

/// Splits `a:'b:c':d` at the colons outside quotes, quotes taken off.
fn args(tag: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut chars = tag.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) if c == '\\' && chars.peek() == Some(&q) => cur.push(chars.next().unwrap_or(q)),
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if (c == '\'' || c == '"') && cur.is_empty() => quote = Some(c),
            None if c == ':' => out.push(std::mem::take(&mut cur)),
            None => cur.push(c),
        }
    }
    out.push(cur);
    out
}

type Resolver<'a> = &'a dyn Fn(&str, &[String]) -> Option<Value>;

struct Frame {
    name: String,
    tag: Option<Tag>,
    children: Vec<Node>,
}

fn close(stack: &mut Vec<Frame>) {
    if stack.len() > 1 {
        let f = stack.pop().unwrap_or(Frame { name: String::new(), tag: None, children: vec![] });
        let node = match f.tag {
            Some(tag) => Node::El(tag, f.children),
            None => return,
        };
        if let Some(parent) = stack.last_mut() {
            parent.children.push(node);
        }
    }
}

fn push_text(stack: &mut [Frame], s: &str) {
    if s.is_empty() {
        return;
    }
    let children = &mut stack.last_mut().expect("root").children;
    if let Some(Node::Text(t)) = children.last_mut() {
        t.push_str(s);
    } else {
        children.push(Node::Text(s.to_owned()));
    }
}

/// The tag `<raw>` as what it opens; None = not a tag (left as text).
fn open_tag(raw: &str, opts: Opts, resolve: Option<Resolver>) -> Option<Result<(String, Tag), Node>> {
    let a = args(raw);
    let name = a[0].to_ascii_lowercase();
    if let Some(key) = decoration(&name) {
        let on = a.get(1).map(|v| v != "false").unwrap_or(true);
        return Some(Ok((name, Tag::Style(key.into(), json!(on)))));
    }
    if let Some(key) = name.strip_prefix('!').and_then(decoration) {
        return Some(Ok((name[1..].to_owned(), Tag::Style(key.into(), json!(false)))));
    }
    if let Some(rgb) = rgb_of(&name) {
        return Some(Ok(("color".into(), Tag::Style("color".into(), color_value(rgb)))));
    }
    match name.as_str() {
        "color" | "colour" | "c" => {
            let rgb = rgb_of(a.get(1)?)?;
            Some(Ok(("color".into(), Tag::Style("color".into(), color_value(rgb)))))
        }
        "gradient" => {
            let mut colors: Vec<u32> = Vec::new();
            let mut phase = 0.0;
            for (i, v) in a.iter().enumerate().skip(1) {
                if let Some(c) = rgb_of(v) {
                    colors.push(c);
                } else if i == a.len() - 1 {
                    phase = v.parse().unwrap_or(0.0);
                }
            }
            if colors.is_empty() {
                colors = vec![0xFFFFFF, 0x000000];
            } else if colors.len() == 1 {
                colors.push(colors[0]);
            }
            Some(Ok((name, Tag::Gradient(colors, phase))))
        }
        "rainbow" => {
            let arg = a.get(1).cloned().unwrap_or_default();
            let reverse = arg.starts_with('!');
            let phase = arg.trim_start_matches('!').parse::<f32>().unwrap_or(0.0);
            Some(Ok((name, Tag::Rainbow(reverse, phase))))
        }
        "transition" => {
            let colors: Vec<u32> = a.iter().skip(1).filter_map(|v| rgb_of(v)).collect();
            let phase: f32 = a.last().and_then(|v| v.parse().ok()).unwrap_or(0.0);
            let rgb = match colors.len() {
                0 => 0xFFFFFF,
                1 => colors[0],
                n => {
                    let t = ((phase + 1.0) / 2.0).clamp(0.0, 1.0) * (n - 1) as f32;
                    let seg = (t.floor() as usize).min(n - 2);
                    lerp(colors[seg], colors[seg + 1], t - seg as f32)
                }
            };
            Some(Ok((name, Tag::Style("color".into(), color_value(rgb)))))
        }
        "hover" => {
            let action = a.get(1)?.to_ascii_lowercase();
            if action != "show_text" {
                return None;
            }
            let value = parse_inner(a.get(2).map(String::as_str).unwrap_or(""), opts, resolve);
            Some(Ok((name, Tag::Style("hover_event".into(), json!({ "action": "show_text", "value": value })))))
        }
        "click" => {
            let action = a.get(1)?.to_ascii_lowercase();
            let value = a[2..].join(":");
            let ev = match action.as_str() {
                "run_command" | "suggest_command" => json!({ "action": action, "command": value }),
                "open_url" => json!({ "action": action, "url": value }),
                "copy_to_clipboard" => json!({ "action": action, "value": value }),
                "change_page" => json!({ "action": action, "page": value.parse::<i64>().unwrap_or(1) }),
                _ => return None,
            };
            Some(Ok((name, Tag::Style("click_event".into(), ev))))
        }
        "insert" | "insertion" => Some(Ok((name, Tag::Style("insertion".into(), json!(a[1..].join(":")))))),
        "font" => Some(Ok((name, Tag::Style("font".into(), json!(a[1..].join(":")))))),
        "shadow" => {
            let rgb = a.get(1).and_then(|v| rgb_of(v)).unwrap_or(0);
            Some(Ok((name, Tag::Style("shadow_color".into(), json!(0xFF00_0000u32 as i64 | rgb as i64)))))
        }
        "key" => Some(Err(Node::Comp(json!({ "keybind": a.get(1)? })))),
        "lang" | "tr" | "translate" => {
            let with: Vec<Value> = a.iter().skip(2).map(|v| parse_inner(v, opts, resolve)).collect();
            Some(Err(Node::Comp(json!({ "translate": a.get(1)?, "with": with }))))
        }
        "newline" | "br" => Some(Err(Node::Text("\n".into()))),
        _ => resolve.and_then(|r| r(&name, &a[1..])).map(|v| Err(Node::Comp(v))),
    }
}

fn parse_inner(input: &str, opts: Opts, resolve: Option<Resolver>) -> Value {
    render(&tree(input, opts, resolve))
}

fn legacy_style(code: char) -> Option<(&'static str, Value)> {
    let code = code.to_ascii_lowercase();
    if let Some(i) = LEGACY_ORDER.find(code) {
        return Some(("color", json!(NAMED[i].0)));
    }
    Some(match code {
        'l' => ("bold", json!(true)),
        'o' => ("italic", json!(true)),
        'n' => ("underlined", json!(true)),
        'm' => ("strikethrough", json!(true)),
        'k' => ("obfuscated", json!(true)),
        _ => return None,
    })
}

fn tree(input: &str, opts: Opts, resolve: Option<Resolver>) -> Vec<Node> {
    let mut stack = vec![Frame { name: String::new(), tag: None, children: vec![] }];
    let chars: Vec<char> = input.chars().collect();
    let mut text = String::new();
    let mut i = 0;
    macro_rules! flush {
        () => {
            push_text(&mut stack, &std::mem::take(&mut text));
        };
    }
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && matches!(chars.get(i + 1), Some('<' | '\\')) {
            text.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if opts.legacy && (c == '&' || c == '§') && i + 1 < chars.len() {
            let next = chars[i + 1];
            // &#rrggbb
            let hex: Option<u32> = (next == '#' && i + 8 <= chars.len())
                .then(|| chars[i + 2..i + 8].iter().collect::<String>())
                .and_then(|h| u32::from_str_radix(&h, 16).ok().filter(|_| h.len() == 6));
            // &x&r&r&g&g&b&b
            let xhex: Option<u32> = (next.eq_ignore_ascii_case(&'x') && i + 14 <= chars.len() && (0..6).all(|k| chars[i + 2 + k * 2] == c))
                .then(|| (0..6).map(|k| chars[i + 3 + k * 2]).collect::<String>())
                .and_then(|h| u32::from_str_radix(&h, 16).ok());
            let style = if let Some(rgb) = hex.or(xhex) { Some(("color", color_value(rgb))) } else { legacy_style(next) };
            let reset = next.eq_ignore_ascii_case(&'r');
            if style.is_some() || reset {
                flush!();
                let is_color = style.as_ref().is_some_and(|(k, _)| *k == "color");
                if is_color || reset {
                    // A colour ends the legacy decorations before it (Bukkit's rule).
                    while stack.last().is_some_and(|f| matches!(f.tag, Some(Tag::Legacy(..)))) {
                        close(&mut stack);
                    }
                }
                if let Some((k, v)) = style {
                    stack.push(Frame { name: "§".into(), tag: Some(Tag::Legacy(k.into(), v)), children: vec![] });
                }
                i += if hex.is_some() { 8 } else if xhex.is_some() { 14 } else { 2 };
                continue;
            }
        }
        if c != '<' {
            text.push(c);
            i += 1;
            continue;
        }
        let mut end = i + 1;
        let mut quote: Option<char> = None;
        while end < chars.len() {
            let e = chars[end];
            match quote {
                Some(q) if e == q => quote = None,
                Some(_) => {}
                None if e == '\'' || e == '"' => quote = Some(e),
                None if e == '>' || e == '<' => break,
                None => {}
            }
            end += 1;
        }
        if end >= chars.len() || chars[end] == '<' || end == i + 1 {
            text.push('<');
            i += 1;
            continue;
        }
        let raw: String = chars[i + 1..end].iter().collect();
        let lower = raw.to_ascii_lowercase();
        if lower == "reset" || lower == "r" {
            flush!();
            while stack.len() > 1 {
                close(&mut stack);
            }
            i = end + 1;
            continue;
        }
        if let Some(closing) = lower.strip_prefix('/') {
            let cname = args(closing)[0].clone();
            let cname = if rgb_of(&cname).is_some() || matches!(cname.as_str(), "colour" | "c") { "color".to_owned() } else { cname.trim_start_matches('!').to_owned() };
            if let Some(at) = stack.iter().rposition(|f| f.name == cname || decoration(&f.name).is_some_and(|d| Some(d) == decoration(&cname))) {
                if at > 0 {
                    flush!();
                    while stack.len() > at {
                        close(&mut stack);
                    }
                    i = end + 1;
                    continue;
                }
            }
            // A closing tag nothing opened: MiniMessage drops it.
            i = end + 1;
            continue;
        }
        match open_tag(&raw, opts, resolve) {
            Some(Ok((name, tag))) => {
                flush!();
                stack.push(Frame { name, tag: Some(tag), children: vec![] });
            }
            Some(Err(node)) => {
                flush!();
                match node {
                    Node::Text(t) => push_text(&mut stack, &t),
                    n => stack.last_mut().expect("root").children.push(n),
                }
            }
            None => text.push_str(&format!("<{raw}>")),
        }
        i = end + 1;
    }
    flush!();
    while stack.len() > 1 {
        close(&mut stack);
    }
    stack.pop().map(|f| f.children).unwrap_or_default()
}

fn count(nodes: &[Node]) -> usize {
    nodes
        .iter()
        .map(|n| match n {
            Node::Text(t) => t.chars().filter(|c| *c != '\n').count(),
            Node::Comp(_) => 1,
            Node::El(_, ch) => count(ch),
        })
        .sum()
}

fn lerp(a: u32, b: u32, t: f32) -> u32 {
    let ch = |x: u32, s: u32| ((x >> s) & 0xFF) as f32;
    let mix = |s: u32| ((ch(a, s) + (ch(b, s) - ch(a, s)) * t).round().clamp(0.0, 255.0) as u32) << s;
    mix(16) | mix(8) | mix(0)
}

fn hsv(h: f32) -> u32 {
    let h = h.rem_euclid(1.0) * 6.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    let (r, g, b) = match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    let c = |v: f32| (v * 255.0).round() as u32;
    (c(r) << 16) | (c(g) << 8) | c(b)
}

#[derive(Clone)]
enum Paint {
    Gradient(Vec<u32>, f32, usize),
    Rainbow(bool, f32, usize),
}

impl Paint {
    fn at(&self, i: usize) -> u32 {
        match self {
            Paint::Gradient(colors, phase, n) => {
                let n = *n;
                if n <= 1 {
                    return colors[0];
                }
                let segs = (colors.len() - 1) as f32;
                let mut t = i as f32 / (n - 1) as f32;
                // A phase shifts the gradient along (and back), as Adventure's does.
                if *phase != 0.0 {
                    t = (t + phase.abs()).rem_euclid(2.0);
                    if t > 1.0 {
                        t = 2.0 - t;
                    }
                    if *phase < 0.0 {
                        t = 1.0 - t;
                    }
                }
                let pos = t * segs;
                let seg = (pos.floor() as usize).min(colors.len() - 2);
                lerp(colors[seg], colors[seg + 1], pos - seg as f32)
            }
            Paint::Rainbow(reverse, phase, n) => {
                let f = i as f32 / (*n).max(1) as f32;
                hsv(if *reverse { 1.0 - f } else { f } + phase / 10.0)
            }
        }
    }
}

struct Out {
    parts: Vec<Value>,
}

impl Out {
    fn push(&mut self, text: &str, style: &Map<String, Value>) {
        if text.is_empty() {
            return;
        }
        if let Some(Value::Object(last)) = self.parts.last_mut() {
            let same = last.len() == style.len() + 1 && style.iter().all(|(k, v)| last.get(k) == Some(v)) && last.get("text").is_some_and(Value::is_string);
            if same {
                let t = format!("{}{}", last["text"].as_str().unwrap_or_default(), text);
                last.insert("text".into(), json!(t));
                return;
            }
        }
        let mut part = style.clone();
        part.insert("text".into(), json!(text));
        self.parts.push(Value::Object(part));
    }
}

fn walk(nodes: &[Node], style: &Map<String, Value>, paint: Option<&Paint>, idx: &mut usize, out: &mut Out) {
    for n in nodes {
        match n {
            Node::Text(t) => match paint {
                Some(p) => {
                    for ch in t.chars() {
                        let mut s = style.clone();
                        if ch != '\n' {
                            s.insert("color".into(), color_value(p.at(*idx)));
                            *idx += 1;
                        }
                        out.push(&ch.to_string(), &s);
                    }
                }
                None => {
                    *idx += t.chars().filter(|c| *c != '\n').count();
                    out.push(t, style);
                }
            },
            Node::Comp(v) => {
                let mut part = style.clone();
                if let Some(p) = paint {
                    part.insert("color".into(), color_value(p.at(*idx)));
                }
                *idx += 1;
                if let Value::Object(m) = v {
                    for (k, val) in m {
                        part.insert(k.clone(), val.clone());
                    }
                } else if let Some(s) = v.as_str() {
                    part.insert("text".into(), json!(s));
                }
                out.parts.push(Value::Object(part));
            }
            Node::El(tag, children) => match tag {
                Tag::Style(k, v) | Tag::Legacy(k, v) => {
                    let mut s = style.clone();
                    s.insert(k.clone(), v.clone());
                    // An explicit colour inside a gradient wins over it.
                    let paint = if k == "color" { None } else { paint };
                    walk(children, &s, paint, idx, out);
                }
                Tag::Gradient(colors, phase) => {
                    let p = Paint::Gradient(colors.clone(), *phase, count(children));
                    let mut local = 0;
                    walk(children, style, Some(&p), &mut local, out);
                    *idx += local;
                }
                Tag::Rainbow(rev, phase) => {
                    let p = Paint::Rainbow(*rev, *phase, count(children));
                    let mut local = 0;
                    walk(children, style, Some(&p), &mut local, out);
                    *idx += local;
                }
            },
        }
    }
}

fn render(nodes: &[Node]) -> Value {
    let mut out = Out { parts: Vec::new() };
    let mut idx = 0;
    walk(nodes, &Map::new(), None, &mut idx, &mut out);
    match out.parts.len() {
        0 => json!({ "text": "" }),
        1 => out.parts.pop().unwrap_or_else(|| json!({ "text": "" })),
        _ => json!({ "text": "", "extra": out.parts }),
    }
}

/// MiniMessage -> text JSON. `<small>text</small>` is small capitals ([`crate::smallcaps`]).
pub fn parse(input: &str) -> Value {
    render(&tree(&crate::smallcaps::apply(input), Opts::default(), None))
}

/// MiniMessage with legacy `&`/`§` codes mixed in -> text JSON.
pub fn parse_mixed(input: &str) -> Value {
    render(&tree(&crate::smallcaps::apply(input), Opts { legacy: true }, None))
}

/// With options and your own tags: `resolve(name, args)` answers `<name:args..>` with a component (or a JSON
/// string), None = not yours.
pub fn parse_with(input: &str, opts: Opts, resolve: &dyn Fn(&str, &[String]) -> Option<Value>) -> Value {
    render(&tree(&crate::smallcaps::apply(input), opts, Some(resolve)))
}

/// Text JSON -> a `§` string (hex as `§x§r§r§g§g§b§b`), for the calls that take legacy text.
pub fn to_legacy(component: &Value) -> String {
    horizon_bukkit::adventure::Component(component.clone()).to_legacy()
}

/// MiniMessage (and legacy codes) -> a `§` string.
pub fn legacy(input: &str) -> String {
    to_legacy(&parse_mixed(input))
}

/// The words alone.
pub fn plain(input: &str) -> String {
    horizon_bukkit::adventure::Component(parse_mixed(input)).plain()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basics() {
        let v = parse("<red>Hi <bold>there</bold>!");
        assert_eq!(v["extra"][0], json!({ "text": "Hi ", "color": "red" }));
        assert_eq!(v["extra"][1], json!({ "text": "there", "color": "red", "bold": true }));
        assert_eq!(v["extra"][2], json!({ "text": "!", "color": "red" }));
    }
    #[test]
    fn gradient_spans() {
        let v = parse("<gradient:#ff0000:#0000ff>abc</gradient>");
        assert_eq!(v["extra"][0]["color"], "#FF0000");
        assert_eq!(v["extra"][2]["color"], "#0000FF");
        assert_eq!(v["extra"][1]["color"], "#800080");
    }
    #[test]
    fn rainbow_and_legacy() {
        let v = parse("<rainbow>ab</rainbow>");
        assert_eq!(v["extra"][0]["color"], "red".replace("red", "#FF0000"));
        let l = parse_mixed("&cRed &lB <green>g");
        assert_eq!(l["extra"][0], json!({ "text": "Red ", "color": "red" }));
        assert_eq!(l["extra"][1]["bold"], true);
        assert_eq!(l["extra"][2]["color"], "green");
    }
    #[test]
    fn hover_click_and_resolver() {
        let v = parse("<hover:show_text:'<red>hi'><click:run_command:'/msg a'>x");
        assert_eq!(v["hover_event"]["value"]["color"], "red");
        assert_eq!(v["click_event"]["command"], "/msg a");
        let r = parse_with("<online>/<max>", Opts::default(), &|n, _| match n {
            "online" => Some(json!("5")),
            "max" => Some(json!("10")),
            _ => None,
        });
        assert_eq!(horizon_bukkit::adventure::Component(r).plain(), "5/10");
        assert_eq!(plain("a <notatag> b"), "a <notatag> b");
    }
}

/// Rewrites what some plugins' configs add on top of MiniMessage into plain MiniMessage first: `#rrggbb`
/// shorthand (no `&`, no `<>`) and colour aliases (`<primary>` -> `<#ff9900>`, closing tags too). Then `parse`.
pub fn preprocess(input: &str, aliases: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // `#rrggbb` not inside a tag and not after `&` or `<`.
        if chars[i] == '#'
            && i + 7 <= chars.len()
            && chars[i + 1..i + 7].iter().all(|c| c.is_ascii_hexdigit())
            && (i == 0 || !matches!(chars[i - 1], '&' | '<' | ':' | '/'))
        {
            out.push('<');
            out.extend(&chars[i..i + 7]);
            out.push('>');
            i += 7;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    for (name, colour) in aliases {
        out = out.replace(&format!("<{name}>"), &format!("<{colour}>")).replace(&format!("</{name}>"), &format!("</{colour}>"));
    }
    out
}

#[cfg(test)]
mod preprocess_tests {
    #[test]
    fn shorthand_and_aliases() {
        assert_eq!(super::preprocess("#ff0000Hi <primary>x</primary>", &[("primary", "#00ff00")]), "<#ff0000>Hi <#00ff00>x</#00ff00>");
        assert_eq!(super::preprocess("&#ff0000 <#00ff00>", &[]), "&#ff0000 <#00ff00>");
    }
}
