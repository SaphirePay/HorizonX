//! LPC 4.4.0 (LuckPerms Chat Formatter) for Horizon, written again in Rust (the Java it follows is in java/).
//! Same `plugins/LPC/config.yml`, same `/lpc`, same permissions.
//!
//! Chat runs on the chat thread, as Paper's `AsyncChatEvent`: settings are one read lock, rank meta comes from
//! LuckPerms-rs's cache through the Vault `Chat` service (map lookups, no inheritance walk), and the line is
//! parsed once and handed to every viewer (Java re-renders per viewer). Pings (sound, action bar) go to the
//! server thread.

use horizon_bukkit::adventure::Component;
use horizon_bukkit::event::{PlayerDeathEvent, PlayerJoinEvent, PlayerQuitEvent};
use horizon_bukkit::fam_ev_player::AsyncChatEvent;
use horizon_bukkit::{CommandSender, Player, Plugin, bukkit_plugin};
use horizon_kit::{Config, hooks, mini};
use regex::Regex;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_CONFIG: &str = include_str!("../data/config.yml");
const DEFAULT_FORMAT: &str = "{prefix}{name}<dark_gray> »<reset> {message}";

static PLUGIN: Mutex<Option<Plugin>> = Mutex::new(None);
static SETTINGS: RwLock<Option<Arc<Settings>>> = RwLock::new(None);
static STATE: LazyLock<Mutex<Moderation>> = LazyLock::new(|| Mutex::new(Moderation::default()));

static URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:https?://)?(?:[a-z0-9-]+\.)+[a-z]{2,}(?:/\S*)?").expect("url"));
static ADVERT_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(?:https?://)?(?:[a-z0-9-]+\.)+[a-z]{2,}(?:/\S*)?").expect("url"));
static IP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:\d{1,3}\.){3}\d{1,3}(?::\d{1,5})?").expect("ip"));
static ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\[item]").expect("item"));

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

#[derive(Default)]
struct Moderation {
    cooldowns: HashMap<String, i64>,
    last: HashMap<String, (String, i64)>,
    /// uuid -> until (ms; i64::MAX = for good).
    mutes: HashMap<String, i64>,
}

#[derive(Clone, Debug, Default)]
struct Settings {
    chat_format: String,
    group_formats: Vec<(String, String)>,
    track_formats: Vec<(String, String)>,
    use_item: bool,
    allow_gradients: bool,
    disabled_worlds: Vec<String>,
    reload_message: String,

    mentions: bool,
    require_at: bool,
    highlight: String,
    max_mentions: usize,
    actionbar: String,
    ping_sound: String,
    ping_volume: f32,
    ping_pitch: f32,

    emoji: bool,
    emoji_perm: bool,
    emojis: Vec<(String, String)>,

    links: bool,
    link_style: String,
    link_hover: String,
    link_schemes: Vec<String>,
    link_max: usize,

    styles: bool,
    group_styles: Vec<(String, String)>,
    track_styles: Vec<(String, String)>,
    default_style: String,

    gradients: bool,
    gradient_key: String,
    group_gradients: Vec<(String, String)>,

    join: bool,
    join_format: String,
    first_join: bool,
    first_join_format: String,
    quit: bool,
    quit_format: String,
    death: bool,
    death_format: String,
    death_vanilla: bool,

    moderation: bool,
    spam: bool,
    spam_ms: i64,
    spam_msg: String,
    repeat: bool,
    repeat_ms: i64,
    repeat_msg: String,
    caps: bool,
    caps_min: usize,
    caps_ratio: f64,
    caps_block: bool,
    caps_msg: String,
    profanity: bool,
    profanity_block: bool,
    profanity_words: Vec<String>,
    profanity_norm: Vec<String>,
    mask_char: char,
    profanity_msg: String,
    advert: bool,
    advert_block: bool,
    advert_ip: bool,
    advert_allow: Vec<String>,
    advert_mask: String,
    advert_msg: String,

    mute: bool,
    mute_commands: bool,
    mute_node: String,
    mute_msg: String,
}

fn section(c: &Config, path: &str) -> Vec<(String, String)> {
    c.keys(path).into_iter().filter_map(|k| c.at(&format!("{path}.{k}")).and_then(|v| v.as_str()).map(|v| (k.clone(), v.to_owned()))).collect()
}

fn load(folder: &std::path::Path) -> Settings {
    let c = Config::load(folder, "config.yml", DEFAULT_CONFIG);
    let words = c.list("profanity-filter.words");
    Settings {
        chat_format: c.str("chat-format", DEFAULT_FORMAT),
        group_formats: section(&c, "group-formats"),
        track_formats: section(&c, "track-formats"),
        use_item: c.bool("use-item-placeholder", false),
        allow_gradients: c.bool("allow-gradient-tags", true),
        disabled_worlds: c.list("disabled-worlds"),
        reload_message: c.str("reload-message", "<green>Reloaded LPC configuration!"),
        mentions: c.bool("mentions.enabled", true),
        require_at: c.bool("mentions.require-at-symbol", false),
        highlight: c.str("mentions.highlight-format", "<gold>@<name></gold>"),
        max_mentions: c.int("mentions.max-mentions-per-message", 5).max(0) as usize,
        actionbar: c.str("mentions.actionbar-text", "<gold>You were mentioned by <white><sender></white>"),
        ping_sound: c.str("mentions.ping-sound", "entity.experience_orb.pickup"),
        ping_volume: c.num("mentions.ping-volume", 1.0) as f32,
        ping_pitch: c.num("mentions.ping-pitch", 1.0) as f32,
        emoji: c.bool("emoji.enabled", true),
        emoji_perm: c.bool("emoji.require-permission", false),
        emojis: section(&c, "emoji.replacements").into_iter().filter(|(k, _)| !k.is_empty()).collect(),
        links: c.bool("link-urls.enabled", true),
        link_style: c.str("link-urls.style", "<blue><underlined>"),
        link_hover: c.str("link-urls.hover-text", "<gray>Open link"),
        link_schemes: {
            let s = c.list("link-urls.allowed-schemes");
            if s.is_empty() { vec!["http".into(), "https".into()] } else { s.into_iter().map(|x| x.to_ascii_lowercase()).collect() }
        },
        link_max: c.int("link-urls.max-length", 200).max(0) as usize,
        styles: c.bool("message-styles.enabled", false),
        group_styles: section(&c, "group-message-styles"),
        track_styles: section(&c, "track-message-styles"),
        default_style: c.str("default-message-style", ""),
        gradients: c.bool("gradient-names.enabled", false),
        gradient_key: c.str("gradient-names.default-meta-key", "username-gradient"),
        group_gradients: section(&c, "group-name-gradients"),
        join: c.bool("join-messages.enabled", false),
        join_format: c.str("join-messages.format", ""),
        first_join: c.bool("join-messages.first-join.enabled", false),
        first_join_format: c.str("join-messages.first-join.format", ""),
        quit: c.bool("quit-messages.enabled", false),
        quit_format: c.str("quit-messages.format", ""),
        death: c.bool("death-messages.enabled", false),
        death_format: c.str("death-messages.format", ""),
        death_vanilla: c.bool("death-messages.show-vanilla-cause", true),
        moderation: c.bool("moderation.enabled", false),
        spam: c.bool("anti-spam.enabled", true),
        spam_ms: c.int("anti-spam.cooldown-millis", 1500),
        spam_msg: c.str("anti-spam.message", "<red>Please wait <yellow><seconds>s</yellow> before chatting again."),
        repeat: c.bool("repeat-filter.enabled", true),
        repeat_ms: c.int("repeat-filter.expiry-millis", 30000),
        repeat_msg: c.str("repeat-filter.message", "<red>Please don't repeat the same message."),
        caps: c.bool("caps-filter.enabled", true),
        caps_min: c.int("caps-filter.min-length", 8).max(0) as usize,
        caps_ratio: c.num("caps-filter.max-uppercase-ratio", 0.7),
        caps_block: c.str("caps-filter.action", "LOWERCASE").eq_ignore_ascii_case("BLOCK"),
        caps_msg: c.str("caps-filter.message", "<red>Please don't shout."),
        profanity: c.bool("profanity-filter.enabled", false),
        profanity_block: c.str("profanity-filter.action", "MASK").eq_ignore_ascii_case("BLOCK"),
        profanity_norm: words.iter().map(|w| normalize(w)).collect(),
        profanity_words: words,
        mask_char: c.str("profanity-filter.mask-char", "*").chars().next().unwrap_or('*'),
        profanity_msg: c.str("profanity-filter.message", "<red>Watch your language."),
        advert: c.bool("anti-advert.enabled", false),
        advert_block: c.str("anti-advert.action", "REDACT").eq_ignore_ascii_case("BLOCK"),
        advert_ip: c.bool("anti-advert.detect-ip", true),
        advert_allow: c.list("anti-advert.allowlist"),
        advert_mask: c.str("anti-advert.mask", "[link removed]"),
        advert_msg: c.str("anti-advert.message", "<red>Advertising is not allowed."),
        mute: c.bool("mute.enabled", false),
        mute_commands: c.bool("mute.enable-commands", true),
        mute_node: c.str("mute.permission-node", "lpc.muted"),
        mute_msg: c.str("mute.message", "<red>You are currently muted."),
    }
}

fn settings() -> Arc<Settings> {
    SETTINGS.read().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}

fn plugin() -> Option<Plugin> {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner())
}

fn reload() {
    let Some(p) = plugin() else { return };
    let s = load(&p.get_data_folder());
    *SETTINGS.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(s));
    hooks::refresh();
}

fn comp(mini_text: &str) -> Component {
    Component(mini::parse(mini_text))
}

/// `<key>` -> plain text (`Placeholder.unparsed`).
fn comp_with(mini_text: &str, key: &str, value: &str) -> Component {
    let v = json!({ "text": value });
    Component(mini::parse_with(mini_text, mini::Opts::default(), &|n, _| (n == key).then(|| v.clone())))
}

// ---- Moderation (TextChecks) ----

fn normalize(input: &str) -> String {
    let mut out = String::new();
    let mut prev = '\0';
    for c in input.to_lowercase().chars() {
        let m = match c {
            '$' | '5' => 's',
            '0' => 'o',
            '1' => 'i',
            '3' => 'e',
            '4' | '@' => 'a',
            '7' => 't',
            other => other,
        };
        if m.is_alphanumeric() {
            if m != prev {
                out.push(m);
                prev = m;
            }
        } else {
            prev = '\0';
        }
    }
    out
}

fn is_shout(text: &str, min: usize, ratio: f64) -> bool {
    let (mut letters, mut upper) = (0usize, 0usize);
    for c in text.chars().filter(|c| c.is_alphabetic()) {
        letters += 1;
        if c.is_uppercase() {
            upper += 1;
        }
    }
    text.chars().count() >= min && letters > 0 && upper as f64 / letters as f64 > ratio
}

fn allowlisted(m: &str, allow: &[String]) -> bool {
    let lower = m.to_lowercase();
    allow.iter().any(|a| !a.is_empty() && lower.contains(&a.to_lowercase()))
}

fn mask_words(text: &str, words: &[String], mask: char) -> String {
    let mut out = text.to_owned();
    for w in words.iter().filter(|w| !w.is_empty()) {
        if let Ok(re) = Regex::new(&format!("(?i){}", regex::escape(w))) {
            out = re.replace_all(&out, mask.to_string().repeat(w.chars().count()).as_str()).into_owned();
        }
    }
    out
}

fn mask_advert(text: &str, s: &Settings) -> String {
    let rep = |re: &Regex, t: &str| re.replace_all(t, |c: &regex::Captures| if allowlisted(&c[0], &s.advert_allow) { c[0].to_owned() } else { s.advert_mask.clone() }).into_owned();
    let out = rep(&ADVERT_URL, text);
    if s.advert_ip { rep(&IP, &out) } else { out }
}

fn has_advert(text: &str, s: &Settings) -> bool {
    ADVERT_URL.find_iter(text).any(|m| !allowlisted(m.as_str(), &s.advert_allow)) || (s.advert_ip && IP.find_iter(text).any(|m| !allowlisted(m.as_str(), &s.advert_allow)))
}

enum Mod {
    Allow,
    Block(Option<Component>),
    Transform(String),
}

fn muted(player: &Player, s: &Settings, now: i64) -> bool {
    if !s.mute {
        return false;
    }
    if !s.mute_node.is_empty() && player.has_permission(&s.mute_node) {
        return true;
    }
    let uuid = player.get_unique_id();
    let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    match st.mutes.get(&uuid).copied() {
        Some(until) if until != i64::MAX && until <= now => {
            st.mutes.remove(&uuid);
            false
        }
        Some(_) => true,
        None => false,
    }
}

fn moderate(player: &Player, raw: &str, s: &Settings) -> Mod {
    let now = now_ms();
    if muted(player, s, now) {
        return Mod::Block(Some(comp(&s.mute_msg)));
    }
    if !s.moderation {
        return Mod::Allow;
    }
    let id = player.get_unique_id();
    if s.spam && !player.has_permission("lpc.bypass.spam") {
        let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(&last) = st.cooldowns.get(&id)
            && now - last < s.spam_ms
        {
            let left = ((s.spam_ms - (now - last) + 999) / 1000).max(1);
            return Mod::Block(Some(comp_with(&s.spam_msg, "seconds", &left.to_string())));
        }
        st.cooldowns.insert(id.clone(), now);
    }
    let norm = normalize(raw);
    if s.repeat && !player.has_permission("lpc.bypass.repeat") {
        let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((text, at)) = st.last.get(&id)
            && now - at < s.repeat_ms
            && *text == norm
        {
            return Mod::Block(Some(comp(&s.repeat_msg)));
        }
        st.last.insert(id.clone(), (norm, now));
    }
    let mut text = raw.to_owned();
    if s.caps && !player.has_permission("lpc.bypass.caps") && is_shout(raw, s.caps_min, s.caps_ratio) {
        if s.caps_block {
            return Mod::Block(Some(comp(&s.caps_msg)));
        }
        text = raw.to_lowercase();
    }
    if s.profanity && !player.has_permission("lpc.bypass.profanity") {
        let n = normalize(&text);
        if s.profanity_norm.iter().any(|w| !w.is_empty() && n.contains(w.as_str())) {
            if s.profanity_block {
                return Mod::Block(Some(comp(&s.profanity_msg)));
            }
            text = mask_words(&text, &s.profanity_words, s.mask_char);
        }
    }
    if s.advert && !player.has_permission("lpc.bypass.advert") && has_advert(&text, s) {
        if s.advert_block {
            return Mod::Block(Some(comp(&s.advert_msg)));
        }
        text = mask_advert(&text, s);
    }
    if text == raw { Mod::Allow } else { Mod::Transform(text) }
}

// ---- The player's message ----

/// Escapes the `<tags>` a player may not use: with `lpc.chatcolor` colours, decorations, reset (and gradients
/// when allowed) stay; click, hover, insertion and everything else become text.
fn sanitize(raw: &str, gradients: bool) -> String {
    let mut out = String::with_capacity(raw.len() + 8);
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<'
            && let Some(len) = chars[i + 1..].iter().position(|c| *c == '>' || *c == '<').filter(|p| chars[i + 1 + p] == '>')
        {
            let tag: String = chars[i + 1..i + 1 + len].iter().collect();
            let name = tag.trim_start_matches('/').trim_start_matches('!').split(':').next().unwrap_or("").to_ascii_lowercase();
            let cosmetic = mini::rgb_of(&name).is_some()
                || matches!(
                    name.as_str(),
                    "color" | "colour" | "c" | "bold" | "b" | "italic" | "i" | "em" | "underlined" | "u" | "strikethrough" | "st" | "obfuscated" | "obf" | "reset" | "r" | "shadow"
                )
                || (gradients && matches!(name.as_str(), "gradient" | "rainbow" | "transition"));
            if cosmetic {
                out.push('<');
                out.push_str(&tag);
                out.push('>');
                i += len + 2;
                continue;
            }
            out.push_str("\\<");
            i += 1;
            continue;
        }
        if chars[i] == '\\' {
            out.push_str("\\\\");
        } else {
            out.push(chars[i]);
        }
        i += 1;
    }
    out
}

/// `&a` / `§a` codes as MiniMessage tags (`LegacyColors.toMiniMessage`).
fn legacy_to_mini(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if (c == '&' || c == '§')
            && let Some(&n) = chars.peek()
        {
            let tag = match n.to_ascii_lowercase() {
                '0' => "black",
                '1' => "dark_blue",
                '2' => "dark_green",
                '3' => "dark_aqua",
                '4' => "dark_red",
                '5' => "dark_purple",
                '6' => "gold",
                '7' => "gray",
                '8' => "dark_gray",
                '9' => "blue",
                'a' => "green",
                'b' => "aqua",
                'c' => "red",
                'd' => "light_purple",
                'e' => "yellow",
                'f' => "white",
                'l' => "bold",
                'o' => "italic",
                'n' => "underlined",
                'm' => "strikethrough",
                'k' => "obfuscated",
                'r' => "reset",
                _ => "",
            };
            if !tag.is_empty() {
                chars.next();
                out.push('<');
                out.push_str(tag);
                out.push('>');
                continue;
            }
        }
        out.push(c);
    }
    out
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('<', "\\<")
}

/// A thing found in the message's text: replaced by a component.
struct Found {
    start: usize,
    end: usize,
    part: Value,
}

struct MessageCtx<'a> {
    s: &'a Settings,
    links: bool,
    mention_re: Option<Regex>,
    names: &'a [String],
    item: Option<Value>,
    mentioned: Vec<String>,
}

fn link_part(s: &Settings, matched: &str) -> Option<Value> {
    let href = if matched.contains("://") { matched.to_owned() } else { format!("https://{matched}") };
    let scheme = href.split("://").next().unwrap_or("").to_ascii_lowercase();
    if matched.chars().count() > s.link_max || !s.link_schemes.contains(&scheme) {
        return None;
    }
    let text = json!({ "text": matched });
    let mut link = mini::parse_with(&format!("{}<url>", s.link_style), mini::Opts::default(), &|n, _| (n == "url").then(|| text.clone()));
    link["click_event"] = json!({ "action": "open_url", "url": href });
    link["hover_event"] = json!({ "action": "show_text", "value": mini::parse(&s.link_hover) });
    Some(link)
}

/// Finds links, mentions and `[item]` in a stretch of plain text; the rest is escaped when it must stay text.
fn text_region(seg: &str, cx: &mut MessageCtx, specials: &mut Vec<Value>, escape_rest: bool) -> String {
    let mut found: Vec<Found> = Vec::new();
    if cx.links {
        for m in URL.find_iter(seg) {
            if let Some(part) = link_part(cx.s, m.as_str()) {
                found.push(Found { start: m.start(), end: m.end(), part });
            }
        }
    }
    if let Some(re) = &cx.mention_re {
        for c in re.captures_iter(seg) {
            let (Some(all), Some(name)) = (c.get(0), c.get(1)) else { continue };
            if cx.mentioned.len() >= cx.s.max_mentions || found.iter().any(|f| all.start() < f.end && f.start < all.end()) {
                continue;
            }
            let canonical = cx.names.iter().find(|n| n.eq_ignore_ascii_case(name.as_str())).cloned().unwrap_or_else(|| name.as_str().to_owned());
            let text = json!({ "text": canonical.clone() });
            let part = mini::parse_with(&cx.s.highlight, mini::Opts::default(), &|n, _| (n == "name").then(|| text.clone()));
            if !cx.mentioned.contains(&canonical) {
                cx.mentioned.push(canonical);
            }
            found.push(Found { start: all.start(), end: all.end(), part });
        }
    }
    if let Some(item) = &cx.item {
        for m in ITEM.find_iter(seg) {
            if !found.iter().any(|f| m.start() < f.end && f.start < m.end()) {
                found.push(Found { start: m.start(), end: m.end(), part: item.clone() });
            }
        }
    }
    found.sort_by_key(|f| f.start);
    let mut out = String::new();
    let mut at = 0;
    for f in found {
        if f.start < at {
            continue;
        }
        let plain = &seg[at..f.start];
        out.push_str(&if escape_rest { escape(plain) } else { plain.to_owned() });
        out.push_str(&format!("<lpcx:{}>", specials.len()));
        specials.push(f.part);
        at = f.end;
    }
    let plain = &seg[at..];
    out.push_str(&if escape_rest { escape(plain) } else { plain.to_owned() });
    out
}

fn item_part(player: &Player) -> Option<Value> {
    let item = player.get_inventory().get_item_in_main_hand()?;
    let key = item.get_type().get_key().to_owned();
    if key.is_empty() || key.ends_with(":air") || key == "air" {
        return None;
    }
    let mut name = match item.display_name() {
        Some(c) => {
            let mut v = c.0;
            strip_interactive(&mut v);
            v
        }
        None => json!({ "text": key.rsplit(':').next().unwrap_or(&key).replace('_', " ") }),
    };
    let id = if key.contains(':') { key.clone() } else { format!("minecraft:{key}") };
    name["hover_event"] = json!({ "action": "show_item", "id": id, "count": item.get_amount() });
    Some(name)
}

/// `ComponentSanitizer.stripInteractive`.
fn strip_interactive(v: &mut Value) {
    if let Some(o) = v.as_object_mut() {
        for k in ["click_event", "hover_event", "insertion", "clickEvent", "hoverEvent"] {
            o.remove(k);
        }
    }
    if let Some(extra) = v.get_mut("extra").and_then(Value::as_array_mut) {
        extra.iter_mut().for_each(strip_interactive);
    }
}

/// The message as players see it, and who it mentions.
fn message(player: &Player, raw: &str, s: &Settings) -> (Value, Vec<String>) {
    let mut text = raw.to_owned();
    if s.emoji && !s.emojis.is_empty() && (!s.emoji_perm || player.has_permission("lpc.emoji")) {
        for (k, v) in &s.emojis {
            text = text.replace(k.as_str(), v);
        }
    }
    let allow_color = player.has_permission("lpc.chatcolor");
    let names: Vec<String> = if s.mentions {
        horizon_bukkit::get_online_players().iter().map(|p| p.get_name().to_owned()).filter(|n| !n.is_empty() && n.len() <= 16).collect()
    } else {
        Vec::new()
    };
    let mention_re = (!names.is_empty() && s.max_mentions > 0).then(|| {
        let alt = names.iter().map(|n| regex::escape(n)).collect::<Vec<_>>().join("|");
        Regex::new(&format!("(?i){}\\b({alt})\\b", if s.require_at { "@" } else { "" })).ok()
    });
    let item = (s.use_item && player.has_permission("lpc.itemplaceholder")).then(|| item_part(player)).flatten();
    let mut cx = MessageCtx { s, links: s.links && player.has_permission("lpc.chatlinks"), mention_re: mention_re.flatten(), names: &names, item, mentioned: Vec::new() };
    let mut specials: Vec<Value> = Vec::new();
    let source = if allow_color { sanitize(&legacy_to_mini(&text), s.allow_gradients) } else { text.clone() };
    let built = if allow_color {
        // Only the stretches between tags are text: links and names are looked for there.
        let mut out = String::new();
        let mut rest = source.as_str();
        while !rest.is_empty() {
            let next_tag = rest.char_indices().find(|&(i, c)| c == '<' && (i == 0 || !rest[..i].ends_with('\\'))).map(|(i, _)| i);
            match next_tag {
                Some(start) => {
                    let end = rest[start..].find('>').map(|e| start + e + 1).unwrap_or(rest.len());
                    out.push_str(&text_region(&rest[..start], &mut cx, &mut specials, false));
                    out.push_str(&rest[start..end]);
                    rest = &rest[end..];
                }
                None => {
                    out.push_str(&text_region(rest, &mut cx, &mut specials, false));
                    rest = "";
                }
            }
        }
        out
    } else {
        text_region(&source, &mut cx, &mut specials, true)
    };
    let value = mini::parse_with(&built, mini::Opts::default(), &|n, a| {
        (n == "lpcx").then(|| a.first().and_then(|i| i.parse::<usize>().ok()).and_then(|i| specials.get(i).cloned())).flatten()
    });
    (value, cx.mentioned)
}

// ---- The format ----

fn by_track(map: &[(String, String)], group: &str) -> Option<String> {
    map.iter().find(|(track, _)| hooks::chat::track_contains(track, group)).map(|(_, v)| v.clone())
}

fn lookup(map: &[(String, String)], key: &str) -> Option<String> {
    map.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.clone())
}

fn gradient_name(player: &Player, meta: &hooks::chat::Meta, s: &Settings) -> String {
    let name = escape(player.get_name());
    if !s.gradients {
        return name;
    }
    let spec = meta.value(&s.gradient_key).map(str::to_owned).filter(|v| !v.trim().is_empty()).or_else(|| lookup(&s.group_gradients, &meta.primary));
    match spec {
        Some(spec) if !spec.is_empty() && spec.chars().all(|c| c.is_ascii_alphanumeric() || "#:,_-".contains(c)) => format!("<gradient:{spec}>{name}</gradient>"),
        _ => name,
    }
}

fn meta_tokens(format: &str, player: &Player, meta: &hooks::chat::Meta, s: &Settings) -> String {
    let display = player.get_display_name();
    let display = if display.is_empty() { player.get_name() } else { display };
    let out = format
        .replace("{prefix}", &meta.prefix)
        .replace("{suffix}", &meta.suffix)
        .replace("{prefixes}", &meta.prefixes.join(" "))
        .replace("{suffixes}", &meta.suffixes.join(" "))
        .replace("{world}", player.get_world().get_name())
        .replace("{gradient-name}", &gradient_name(player, meta, s))
        .replace("{name}", &escape(player.get_name()))
        .replace("{displayname}", &escape(display))
        .replace("{username-color}", meta.value("username-color").unwrap_or(""))
        .replace("{message-color}", meta.value("message-color").unwrap_or(""));
    hooks::placeholders(&out, Some(player))
}

/// A template (join, quit, death) for a player, with extra `<tags>`.
fn template(player: &Player, format: &str, s: &Settings, extra: &[(&str, Value)]) -> Value {
    let meta = hooks::chat::all(&player.get_unique_id());
    let text = meta_tokens(format, player, &meta, s);
    mini::parse_with(&text, mini::Opts { legacy: true }, &|n, _| extra.iter().find(|(k, _)| *k == n).map(|(_, v)| v.clone()))
}

fn render(player: &Player, msg: Value, s: &Settings) -> Value {
    let meta = hooks::chat::all(&player.get_unique_id());
    let group = if meta.primary.is_empty() { "default".to_owned() } else { meta.primary.clone() };
    let styled = if s.styles {
        let style = lookup(&s.group_styles, &group).or_else(|| by_track(&s.track_styles, &group)).unwrap_or_else(|| s.default_style.clone());
        if style.is_empty() {
            msg
        } else {
            let inner = msg.clone();
            mini::parse_with(&style.replace("{message}", "<lpcstyled>"), mini::Opts::default(), &|n, _| (n == "lpcstyled").then(|| inner.clone()))
        }
    } else {
        msg
    };
    let format = lookup(&s.group_formats, &group).or_else(|| by_track(&s.track_formats, &group)).unwrap_or_else(|| s.chat_format.clone());
    let format = if format.is_empty() { DEFAULT_FORMAT.to_owned() } else { format };
    let format = meta_tokens(&format.replace("{message}", "<lpcmsg>"), player, &meta, s);
    mini::parse_with(&format, mini::Opts { legacy: true }, &|n, _| (n == "lpcmsg").then(|| styled.clone()))
}

fn ping(target: &Player, sender: &str, s: &Settings) {
    if target.has_permission("lpc.mention.exempt") {
        return;
    }
    if !s.ping_sound.is_empty() {
        target.play_sound(&target.get_location(), &s.ping_sound, s.ping_volume, s.ping_pitch);
    }
    if !s.actionbar.is_empty() {
        target.send_action_bar(&comp_with(&s.actionbar, "sender", sender));
    }
}

fn on_chat(event: &mut AsyncChatEvent) {
    let player = event.get_player().clone();
    let s = settings();
    let world = player.get_world().get_name().to_owned();
    if s.disabled_worlds.iter().any(|w| w.eq_ignore_ascii_case(&world)) {
        return;
    }
    let raw = event.message().plain();
    let raw = match moderate(&player, &raw, &s) {
        Mod::Block(notice) => {
            event.set_cancelled(true);
            if let Some(n) = notice {
                player.send_component(&n);
            }
            return;
        }
        Mod::Transform(t) => t,
        Mod::Allow => raw,
    };
    if s.use_item && !player.has_permission("lpc.itemplaceholder") && ITEM.is_match(&raw) {
        player.send_component(&comp(
            "<dark_gray>[<gradient:#B754F4:#FC00FF>LPC</gradient>] <yellow>Ask an admin for the <white>lpc.itemplaceholder</white> permission to use <white>[item]</white> in chat.",
        ));
    }
    let (msg, mentioned) = message(&player, &raw, &s);
    let line = Component(render(&player, msg.clone(), &s));
    event.set_message(&Component(msg));
    event.renderer(move |_source, _name, _message, _viewer| line.clone());
    if s.mentions && !mentioned.is_empty()
        && let Some(p) = plugin()
    {
        let sender = player.get_name().to_owned();
        let s2 = s.clone();
        p.run_task(move || {
            for name in &mentioned {
                if let Some(target) = horizon_bukkit::get_player(name).filter(|t| t.get_name().eq_ignore_ascii_case(name)) {
                    ping(&target, &sender, &s2);
                }
            }
        });
    }
}

fn on_join(event: &mut PlayerJoinEvent) {
    let s = settings();
    if !s.join {
        return;
    }
    let player = event.get_player().clone();
    let first = s.first_join && !player.has_played_before();
    let format = if first { &s.first_join_format } else { &s.join_format };
    if format.is_empty() {
        event.set_join_message_component(None);
    } else {
        event.set_join_message_component(Some(&Component(template(&player, format, &s, &[]))));
    }
}

fn on_quit(event: &mut PlayerQuitEvent) {
    let s = settings();
    if !s.quit {
        return;
    }
    let player = event.get_player().clone();
    if s.quit_format.is_empty() {
        event.set_quit_message_component(None);
    } else {
        event.set_quit_message_component(Some(&Component(template(&player, &s.quit_format, &s, &[]))));
    }
}

fn on_death(event: &mut PlayerDeathEvent) {
    let s = settings();
    if !s.death {
        return;
    }
    let player = event.get_player().clone();
    let cause = if s.death_vanilla {
        event.get_death_message().map_or_else(|| json!({ "text": "" }), |t| horizon_bukkit::adventure::legacy(t).0)
    } else {
        json!({ "text": "" })
    };
    if s.death_format.is_empty() {
        event.set_death_message(None);
    } else {
        // The engine says death messages as legacy text.
        event.set_death_message(Some(&mini::to_legacy(&template(&player, &s.death_format, &s, &[("death-message", cause)]))));
    }
}

// ---- /lpc ----

fn parse_duration(input: &str) -> i64 {
    let t = input.trim();
    let Some(unit) = t.chars().last() else { return 0 };
    if unit.is_ascii_digit() {
        return t.parse::<i64>().map_or(0, |n| n * 1000);
    }
    let Ok(amount) = t[..t.len() - unit.len_utf8()].trim().parse::<i64>() else { return 0 };
    amount
        * match unit.to_ascii_lowercase() {
            'd' => 86_400_000,
            'h' => 3_600_000,
            'm' => 60_000,
            's' => 1000,
            _ => 0,
        }
}

fn send(sender: &CommandSender, mini_text: &str) {
    sender.send_component(&comp(mini_text));
}

fn command(sender: &CommandSender, args: &[&str]) -> bool {
    let s = settings();
    let no_perm = "<red>You don't have permission to do that.";
    match args.first().map(|a| a.to_ascii_lowercase()).as_deref() {
        Some("reload") => {
            if !sender.has_permission("lpc.reload") {
                send(sender, no_perm);
            } else {
                reload();
                send(sender, &settings().reload_message);
            }
        }
        Some("version") => {
            send(sender, &format!("<gradient:#B754F4:#FC00FF>LPC</gradient> <gray>v<white>{}</white> <dark_gray>— <gray>MiniMessage chat formatter.", env!("CARGO_PKG_VERSION")));
            send(sender, "<dark_gray>Build: <gray>native Rust for <white>Horizon</white>");
            send(sender, "<dark_gray>Running on: <white>Horizon</white>");
        }
        Some(sub @ ("mute" | "unmute")) => {
            if !sender.has_permission("lpc.mute") {
                send(sender, no_perm);
                return true;
            }
            if sub == "mute" && !(s.mute && s.mute_commands) {
                send(sender, "<red>Mute commands are disabled in the config.");
                return true;
            }
            let Some(name) = args.get(1) else {
                send(sender, if sub == "mute" { "<red>Usage: /lpc mute <player> [duration e.g. 10m]" } else { "<red>Usage: /lpc unmute <player>" });
                return true;
            };
            let Some(target) = horizon_bukkit::get_player(name).filter(|p| p.get_name().eq_ignore_ascii_case(name)) else {
                sender.send_component(&comp_with("<red>Player <white><name></white> is not online.", "name", name));
                return true;
            };
            let uuid = target.get_unique_id();
            if sub == "mute" {
                let duration = args.get(2).map_or(0, |d| parse_duration(d));
                let until = if duration <= 0 { i64::MAX } else { now_ms() + duration };
                STATE.lock().unwrap_or_else(|e| e.into_inner()).mutes.insert(uuid, until);
                let suffix = if duration > 0 { format!(" for {}", args[2]) } else { " permanently".to_owned() };
                sender.send_component(&comp_with(&format!("<green>Muted <white><name></white>{suffix}."), "name", target.get_name()));
            } else {
                STATE.lock().unwrap_or_else(|e| e.into_inner()).mutes.remove(&uuid);
                sender.send_component(&comp_with("<green>Unmuted <white><name></white>.", "name", target.get_name()));
            }
        }
        _ => {
            send(sender, "<gradient:#B754F4:#FC00FF>LPC</gradient> <gray>commands:");
            send(sender, "<dark_gray>- <white>/lpc reload</white> <dark_gray>» <gray>Reload the configuration");
            send(sender, "<dark_gray>- <white>/lpc version</white> <dark_gray>» <gray>Show the plugin version");
            if s.mute && s.mute_commands {
                send(sender, "<dark_gray>- <white>/lpc mute \\<player> [duration]</white> <dark_gray>» <gray>Mute a player");
                send(sender, "<dark_gray>- <white>/lpc unmute \\<player></white> <dark_gray>» <gray>Unmute a player");
            }
        }
    }
    true
}

fn complete(args: &[&str]) -> Vec<String> {
    let typed = |i: usize| args.get(i).map(|a| a.to_ascii_lowercase()).unwrap_or_default();
    match args.len() {
        0 | 1 => ["reload", "version", "help", "mute", "unmute"].iter().filter(|s| s.starts_with(&typed(0))).map(|s| (*s).to_owned()).collect(),
        2 if matches!(typed(0).as_str(), "mute" | "unmute") => {
            horizon_bukkit::get_online_players().iter().map(|p| p.get_name().to_owned()).filter(|n| n.to_ascii_lowercase().starts_with(&typed(1))).collect()
        }
        _ => Vec::new(),
    }
}

fn on_enable(plugin: Plugin) {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(plugin);
    reload();
    plugin.register_event_with("NORMAL", false, on_chat);
    plugin.register_event_with("NORMAL", false, on_join);
    plugin.register_event_with("NORMAL", false, on_quit);
    plugin.register_event_with("NORMAL", false, on_death);
    plugin.set_executor("lpc", |sender, _, args| command(sender, args));
    plugin.set_tab_completer("lpc", |_, _, args| complete(args));
    plugin.info("Running on Horizon [native Rust, Adventure chat]");
}

fn on_disable(_plugin: Plugin) {
    *SETTINGS.write().unwrap_or_else(|e| e.into_inner()) = None;
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

bukkit_plugin!(on_enable, on_disable);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checks() {
        assert_eq!(normalize("H3ll0  w0rld!!"), "helo" .to_owned() + "world");
        assert!(is_shout("HELLO THERE", 8, 0.7));
        assert!(!is_shout("Hello there", 8, 0.7));
        assert_eq!(parse_duration("10m"), 600_000);
        assert_eq!(parse_duration("5"), 5000);
        assert_eq!(sanitize("<red>hi <click:run_command:/op me>x", true), "<red>hi \\<click:run_command:/op me>x");
        assert_eq!(legacy_to_mini("&cHi"), "<red>Hi");
        let s = Settings { advert_mask: "[x]".into(), advert_ip: true, ..Default::default() };
        assert_eq!(mask_advert("join play.example.com now 1.2.3.4", &s), "join [x] now [x]");
        assert_eq!(mask_words("you DUMB", &["dumb".into()], '*'), "you ****");
    }
    #[test]
    fn default_config_reads() {
        let c = Config::parse(DEFAULT_CONFIG);
        assert_eq!(c.str("chat-format", ""), DEFAULT_FORMAT);
        assert_eq!(section(&c, "emoji.replacements").len(), 6);
    }
}
