//! The game's own dialog screens from a plugin, in a few lines, with the clicks handled by closures: real text
//! boxes, sliders, toggles and choice buttons instead of "type it in chat", and clean notices and confirmations.
//! Text is MiniMessage. `install(plugin)` once in `on_enable` routes the clicks.
//!
//! ```ignore
//! use horizon_kit::dialog::{self, Dialog};
//! Dialog::new("Create a Warp")
//!     .text("Pick a name and who may visit.")
//!     .input_text("name", "Name", "", 16)
//!     .toggle("public", "Public", true)
//!     .slider("cost", "Visit cost", 0.0, 1000.0, 50.0, 0.0)
//!     .button("<#4ade80>Create", |p, form| create(p, form.text("name"), form.flag("public"), form.number("cost")))
//!     .cancel("Cancel")
//!     .open(&player);
//!
//! dialog::confirm(&player, "Delete your shop?", "This can't be undone.", "Delete", |p| delete(p));
//! dialog::ask_number(&player, "Sell price", "Price each", 1.0, 10_000.0, 1.0, 100.0, |p, price| set(p, price));
//! ```
//!
//! Every server dialog also carries a small ⚠ drawn by the game (it marks the screen as the server's); it can't be
//! taken away.

use crate::theme::{ACCENT, BAD, GOOD, MUTED, TEXT};
use horizon_bukkit::fam_ev_player::PlayerCustomClickEvent;
use horizon_bukkit::{ItemStack, Player, Plugin};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

type Callback = Arc<dyn Fn(&Player, &Form) + Send + Sync>;

/// What the player filled in, by input key.
#[derive(Clone, Debug, Default)]
pub struct Form(HashMap<String, Value>);

impl Form {
    pub fn text(&self, key: &str) -> String {
        match self.0.get(key) {
            Some(Value::String(s)) => s.clone(),
            Some(v) if !v.is_null() => v.to_string(),
            _ => String::new(),
        }
    }
    pub fn number(&self, key: &str) -> f64 {
        match self.0.get(key) {
            Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
            Some(Value::String(s)) => s.trim().parse().unwrap_or(0.0),
            _ => 0.0,
        }
    }
    pub fn flag(&self, key: &str) -> bool {
        match self.0.get(key) {
            Some(Value::Bool(b)) => *b,
            Some(Value::Number(n)) => n.as_i64().unwrap_or(0) != 0,
            Some(Value::String(s)) => s == "true" || s == "1",
            _ => false,
        }
    }
    /// The id of the chosen option of a `choice`.
    pub fn choice(&self, key: &str) -> String {
        self.text(key)
    }
    pub fn raw(&self) -> &HashMap<String, Value> {
        &self.0
    }
}

static NEXT: AtomicU64 = AtomicU64::new(1);
/// Live callbacks: action id -> (player uuid, callback). A player's earlier dialog's callbacks go when they open
/// another.
static CALLBACKS: Mutex<Option<HashMap<String, (String, Callback)>>> = Mutex::new(None);
static NAMESPACE: Mutex<String> = Mutex::new(String::new());
static PACK_HIDES_WARNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Tells the kit the server pack blanks the game's warning button (gui/sprites/dialog/warning_button*): the game
/// still keeps its 20 px spot right of the title, so titles get a matching lead-in to sit centred.
pub fn pack_hides_warning(on: bool) {
    PACK_HIDES_WARNING.store(on, Ordering::Relaxed);
}

/// The widest line of some MiniMessage in the game's default font, in GUI pixels (tags skipped; `<b>` adds 1 a glyph).
pub fn text_width(mini: &str) -> i32 {
    mini.split('\n')
        .map(|line| {
            let (mut w, mut bold, mut tag, mut name) = (0, false, false, String::new());
            for c in line.chars() {
                if tag {
                    if c == '>' {
                        tag = false;
                        match name.as_str() {
                            "b" | "bold" => bold = true,
                            "/b" | "/bold" | "reset" => bold = false,
                            _ => {}
                        }
                    } else {
                        name.push(c);
                    }
                    continue;
                }
                if c == '<' {
                    tag = true;
                    name.clear();
                    continue;
                }
                let g = match c {
                    'i' | '!' | '.' | ',' | ':' | ';' | '|' | '\'' => 2,
                    'l' | '`' => 3,
                    't' | 'I' | ' ' | '[' | ']' | '(' | ')' => 4,
                    'f' | 'k' | '<' | '>' | '{' | '}' | '"' | '*' => 5,
                    '@' | '~' => 7,
                    _ => 6,
                };
                w += g + i32::from(bold);
            }
            w
        })
        .max()
        .unwrap_or(0)
        + 2
}

/// The text box for an item's line: the text's width plus room so the game never wraps it.
fn fitted(mini: &str) -> i32 {
    (text_width(mini) * 11 / 10 + 12).clamp(24, 280)
}

/// The title as sent: with the warning button hidden, 8 plain spaces (32 px, the button and its gap) before it.
fn sent_title(title: &str) -> String {
    if title.is_empty() || !PACK_HIDES_WARNING.load(Ordering::Relaxed) { title.to_owned() } else { format!("{}{title}", " ".repeat(8)) }
}

fn namespace() -> String {
    let ns = NAMESPACE.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if ns.is_empty() { "kit".into() } else { ns }
}

/// Routes the dialogs' clicks to their closures. Call once in `on_enable`.
pub fn install(plugin: Plugin) {
    let ns: String = plugin.get_name().to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    *NAMESPACE.lock().unwrap_or_else(|e| e.into_inner()) = ns;
    plugin.register_event_with("NORMAL", false, |e: &mut PlayerCustomClickEvent| {
        let Some(key) = e.get_identifier() else { return };
        let id = format!("{}:{}", key.get_namespace(), key.get_key());
        let Some(p) = e.get_player() else { return };
        let found = CALLBACKS.lock().unwrap_or_else(|x| x.into_inner()).as_ref().and_then(|m| m.get(&id).cloned());
        let Some((owner, cb)) = found else { return };
        if owner != p.get_unique_id() {
            return;
        }
        let form = Form(e.get_dialog_response_view().and_then(|v| v.0.as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())).unwrap_or_default());
        crate::sync::on_main(move || cb(&p, &form));
    });
}

fn register(player: &Player, cb: Callback) -> String {
    let id = format!("{}:dialog/{}", namespace(), NEXT.fetch_add(1, Ordering::Relaxed));
    CALLBACKS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(id.clone(), (player.get_unique_id(), cb));
    id
}

fn forget(player: &Player) {
    let me = player.get_unique_id();
    if let Some(m) = CALLBACKS.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        m.retain(|_, (owner, _)| *owner != me);
    }
}

/// A player left: their dialogs' callbacks go.
pub fn forget_player(uuid: &str) {
    if let Some(m) = CALLBACKS.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        m.retain(|_, (owner, _)| owner != uuid);
    }
}

fn text(mini: &str) -> Value {
    crate::mini::parse(mini)
}

enum Act {
    Callback(Callback),
    Url(String),
    Close,
    Command(String),
}

struct Button {
    label: String,
    tooltip: Option<String>,
    width: i32,
    act: Act,
}

/// A dialog being put together.
pub struct Dialog {
    title: String,
    body: Vec<Value>,
    inputs: Vec<Value>,
    buttons: Vec<Button>,
    exit: Option<Button>,
    columns: i32,
    width: i32,
    escape: bool,
}

impl Dialog {
    /// A dialog with a title (MiniMessage; empty for none).
    pub fn new(title: &str) -> Self {
        Dialog { title: title.to_owned(), body: Vec::new(), inputs: Vec::new(), buttons: Vec::new(), exit: None, columns: 1, width: 200, escape: true }
    }
    /// A paragraph (MiniMessage; `\n` breaks lines).
    pub fn text(mut self, mini: &str) -> Self {
        self.body.push(json!({ "type": "minecraft:plain_message", "contents": text(&format!("<{MUTED}>{mini}")), "width": 280 }));
        self
    }
    /// A headline line, bold and light.
    pub fn headline(mut self, mini: &str) -> Self {
        self.body.push(json!({ "type": "minecraft:plain_message", "contents": text(&format!("<{TEXT}><b>{mini}")), "width": 280 }));
        self
    }
    /// An item shown in the body, with a line beside it.
    pub fn item(mut self, stack: &ItemStack, beside: &str) -> Self {
        let mut v = json!({ "type": "minecraft:item", "item": stack_json(stack), "show_decorations": true, "show_tooltip": true, "width": 16, "height": 16 });
        if !beside.is_empty() {
            v["description"] = json!({ "contents": text(&format!("<{MUTED}>{beside}")), "width": fitted(beside) });
        }
        self.body.push(v);
        self
    }
    /// A thin solid line across, to separate parts (struck-through spaces, so it has no gaps).
    pub fn divider(mut self) -> Self {
        self.body.push(json!({ "type": "minecraft:plain_message", "contents": text(&format!("<#3f3f46><st>{}", " ".repeat(56))), "width": 280 }));
        self
    }
    /// Any MiniMessage line, as given, at a width.
    pub fn line(mut self, mini: &str, width: i32) -> Self {
        self.body.push(json!({ "type": "minecraft:plain_message", "contents": text(mini), "width": width.clamp(1, 1024) }));
        self
    }
    /// An item card: the item, and beside it a few lines (MiniMessage; `\n` breaks lines).
    pub fn card(mut self, stack: &ItemStack, lines: &str) -> Self {
        // The game centres item + text box as one, so the box is sized to the text: the item then sits right beside it.
        let width = fitted(lines);
        self.body.push(json!({ "type": "minecraft:item", "item": stack_json(stack), "show_decorations": false, "show_tooltip": false, "width": 16, "height": 16, "description": { "contents": text(lines), "width": width } }));
        self
    }
    /// A slider whose value reads as `format` (`%1$s` = the label, `%2$s` = the value): "%2$s coins".
    #[allow(clippy::too_many_arguments)]
    pub fn slider_as(mut self, key: &str, label: &str, min: f64, max: f64, step: f64, initial: f64, format: &str) -> Self {
        let mut v = json!({ "type": "minecraft:number_range", "key": key, "label": text(label), "start": min, "end": max, "initial": initial.clamp(min, max), "width": 200, "label_format": format });
        if step > 0.0 {
            v["step"] = json!(step);
        }
        self.inputs.push(v);
        self
    }
    /// A text box (`max` characters; 0 = the game's 32).
    pub fn input_text(mut self, key: &str, label: &str, initial: &str, max: i32) -> Self {
        let mut v = json!({ "type": "minecraft:text", "key": key, "label": text(label), "initial": initial, "width": 200 });
        if max > 0 {
            v["max_length"] = json!(max);
        }
        self.inputs.push(v);
        self
    }
    /// A text box at a width.
    pub fn input_text_at(mut self, key: &str, label: &str, initial: &str, max: i32, width: i32) -> Self {
        let mut v = json!({ "type": "minecraft:text", "key": key, "label": text(label), "initial": initial, "width": width.clamp(1, 1024) });
        if max > 0 {
            v["max_length"] = json!(max);
        }
        self.inputs.push(v);
        self
    }
    /// A text box several lines tall.
    pub fn input_paragraph(mut self, key: &str, label: &str, initial: &str, lines: i32) -> Self {
        self.inputs.push(json!({ "type": "minecraft:text", "key": key, "label": text(label), "initial": initial, "width": 260, "max_length": 512, "multiline": { "max_lines": lines.max(2), "height": 18 * lines.max(2) } }));
        self
    }
    /// A slider from `min` to `max` in steps of `step`.
    pub fn slider(mut self, key: &str, label: &str, min: f64, max: f64, step: f64, initial: f64) -> Self {
        let mut v = json!({ "type": "minecraft:number_range", "key": key, "label": text(label), "start": min, "end": max, "initial": initial.clamp(min, max), "width": 200, "label_format": "options.generic_value" });
        if step > 0.0 {
            v["step"] = json!(step);
        }
        self.inputs.push(v);
        self
    }
    /// An on/off checkbox.
    pub fn toggle(mut self, key: &str, label: &str, initial: bool) -> Self {
        self.inputs.push(json!({ "type": "minecraft:boolean", "key": key, "label": text(label), "initial": initial, "on_true": "true", "on_false": "false" }));
        self
    }
    /// A button that cycles through options: (id, label); `initial` = the id shown first.
    pub fn choice(mut self, key: &str, label: &str, options: &[(&str, &str)], initial: &str) -> Self {
        let opts: Vec<Value> = options.iter().map(|(id, l)| json!({ "id": id, "display": text(l), "initial": *id == initial })).collect();
        self.inputs.push(json!({ "type": "minecraft:single_option", "key": key, "label": text(label), "options": opts, "width": 200 }));
        self
    }
    /// A button that runs `on_click` with what was filled in.
    pub fn button(mut self, label: &str, on_click: impl Fn(&Player, &Form) + Send + Sync + 'static) -> Self {
        self.buttons.push(Button { label: label.to_owned(), tooltip: None, width: 150, act: Act::Callback(Arc::new(on_click)) });
        self
    }
    /// A button with a tooltip.
    pub fn button_tip(mut self, label: &str, tooltip: &str, on_click: impl Fn(&Player, &Form) + Send + Sync + 'static) -> Self {
        self.buttons.push(Button { label: label.to_owned(), tooltip: Some(tooltip.to_owned()), width: 150, act: Act::Callback(Arc::new(on_click)) });
        self
    }
    /// A button that opens a link (the game asks first).
    pub fn link(mut self, label: &str, url: &str) -> Self {
        self.buttons.push(Button { label: label.to_owned(), tooltip: Some(url.to_owned()), width: 150, act: Act::Url(url.to_owned()) });
        self
    }
    /// A button that runs a command as the player (`/` optional).
    pub fn command(mut self, label: &str, command: &str) -> Self {
        self.buttons.push(Button { label: label.to_owned(), tooltip: None, width: 150, act: Act::Command(command.trim_start_matches('/').to_owned()) });
        self
    }
    /// The button at the bottom that closes it (Escape does the same).
    pub fn cancel(mut self, label: &str) -> Self {
        self.exit = Some(Button { label: label.to_owned(), tooltip: None, width: 150, act: Act::Close });
        self
    }
    /// Buttons per row (1 = stacked).
    pub fn columns(mut self, n: i32) -> Self {
        self.columns = n.max(1);
        self
    }
    /// Every button's width in pixels.
    pub fn button_width(mut self, px: i32) -> Self {
        self.width = px.clamp(40, 1024);
        self
    }
    /// Escape can't close it (only its buttons).
    pub fn locked(mut self) -> Self {
        self.escape = false;
        self
    }

    fn button_json(&self, player: &Player, b: &Button, has_inputs: bool) -> Value {
        let mut v = json!({ "label": text(&b.label), "width": if b.width == 150 { self.width } else { b.width } });
        if let Some(t) = &b.tooltip {
            v["tooltip"] = text(t);
        }
        match &b.act {
            Act::Callback(cb) => {
                let id = register(player, cb.clone());
                // With inputs, the click sends them too.
                v["action"] = if has_inputs { json!({ "type": "dynamic/custom", "id": id }) } else { json!({ "type": "custom", "id": id }) };
            }
            Act::Url(u) => v["action"] = json!({ "type": "open_url", "url": u }),
            Act::Command(c) => v["action"] = json!({ "type": "run_command", "command": format!("/{c}") }),
            Act::Close => {}
        }
        v
    }

    /// The dialog as the game reads it (for this player: the callbacks are theirs).
    pub fn build(&self, player: &Player) -> Value {
        let has_inputs = !self.inputs.is_empty();
        let mut d = json!({
            "title": text(&sent_title(&self.title)),
            "body": self.body,
            "can_close_with_escape": self.escape,
            "pause": false,
            "after_action": "close",
        });
        if has_inputs {
            d["inputs"] = json!(self.inputs);
        }
        let buttons: Vec<Value> = self.buttons.iter().map(|b| self.button_json(player, b, has_inputs)).collect();
        let exit = self.exit.as_ref().map(|b| self.button_json(player, b, false));
        match (buttons.len(), &exit) {
            (0, None) => {
                d["type"] = json!("minecraft:notice");
            }
            (1, None) => {
                d["type"] = json!("minecraft:notice");
                d["action"] = buttons[0].clone();
            }
            (1, Some(no)) => {
                d["type"] = json!("minecraft:confirmation");
                d["yes"] = buttons[0].clone();
                d["no"] = no.clone();
            }
            _ => {
                d["type"] = json!("minecraft:multi_action");
                d["actions"] = json!(buttons);
                d["columns"] = json!(self.columns);
                if let Some(x) = exit {
                    d["exit_action"] = x;
                }
            }
        }
        d
    }

    /// Shows it to the player (their earlier dialog's buttons stop working).
    pub fn open(self, player: &Player) {
        forget(player);
        let d = self.build(player);
        horizon_bukkit::horizon_mod::run_command(&format!("dialog show {} {}", player.get_name(), horizon_bukkit::data::snbt(&d)));
    }
}

fn stack_json(stack: &ItemStack) -> Value {
    json!({ "id": stack.get_type().key(), "count": stack.get_amount().max(1) })
}

/// Closes whatever dialog the player has open.
pub fn close(player: &Player) {
    forget(player);
    horizon_bukkit::horizon_mod::run_command(&format!("dialog clear {}", player.get_name()));
}

// ---------------------------------------------------------------- ready-made

/// A message with one OK button.
pub fn notice(player: &Player, title: &str, message: &str) {
    Dialog::new(title).text(message).button("OK", |p, _| close(p)).open(player);
}

/// Yes / No. `yes` runs on yes.
pub fn confirm(player: &Player, question: &str, detail: &str, yes_label: &str, yes: impl Fn(&Player) + Send + Sync + 'static) {
    let mut d = Dialog::new("").headline(question);
    if !detail.is_empty() {
        d = d.text(detail);
    }
    d.button(&format!("<{GOOD}>{yes_label}"), move |p, _| yes(p)).cancel(&format!("<{BAD}>Cancel")).open(player);
}

/// A line of text from the player (a name, a message...). Empty answers are ignored.
pub fn ask_text(player: &Player, title: &str, label: &str, initial: &str, max: i32, then: impl Fn(&Player, String) + Send + Sync + 'static) {
    Dialog::new(title)
        .input_text("value", label, initial, max)
        .button(&format!("<{GOOD}>Done"), move |p, f| {
            let v = f.text("value").trim().to_owned();
            if !v.is_empty() {
                then(p, v);
            }
        })
        .cancel(&format!("<{MUTED}>Cancel"))
        .open(player);
}

/// A number from the player, on a slider.
#[allow(clippy::too_many_arguments)]
pub fn ask_number(player: &Player, title: &str, label: &str, min: f64, max: f64, step: f64, initial: f64, then: impl Fn(&Player, f64) + Send + Sync + 'static) {
    Dialog::new(title)
        .slider("value", label, min, max, step, initial)
        .button(&format!("<{GOOD}>Done"), move |p, f| then(p, f.number("value")))
        .cancel(&format!("<{MUTED}>Cancel"))
        .open(player);
}

/// One of several choices, as a column of buttons: (id, label).
pub fn choose(player: &Player, title: &str, message: &str, options: &[(&str, &str)], then: impl Fn(&Player, &str) + Send + Sync + 'static) {
    let then = Arc::new(then);
    let mut d = Dialog::new(title);
    if !message.is_empty() {
        d = d.text(message);
    }
    for (id, label) in options {
        let id = (*id).to_owned();
        let then = then.clone();
        d = d.button(&format!("<{ACCENT}>{label}"), move |p, _| then(p, &id));
    }
    d.cancel(&format!("<{MUTED}>Cancel")).open(player);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_values() {
        let f = Form([("n".to_owned(), json!(12.5)), ("s".to_owned(), json!("abc")), ("b".to_owned(), json!(true)), ("t".to_owned(), json!("1"))].into());
        assert_eq!(f.number("n"), 12.5);
        assert_eq!(f.text("s"), "abc");
        assert!(f.flag("b") && f.flag("t") && !f.flag("missing"));
        assert_eq!(f.text("missing"), "");
    }
}
