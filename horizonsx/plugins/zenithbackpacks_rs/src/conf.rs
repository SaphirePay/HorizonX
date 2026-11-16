//! config.yml (the row ladder, the item, the `chest:` block), gui.yml and messages.yml, read as the Java read
//! them: the file's keys first, the plugin's own defaults behind them.

use horizon_kit::Config;
use horizon_kit::mini;
use serde_json::Value;
use serde_yaml::Value as Yaml;
use std::path::Path;

pub const DEFAULT_CONFIG: &str = include_str!("../data/config.yml");
pub const DEFAULT_GUI: &str = include_str!("../data/gui.yml");
pub const DEFAULT_MESSAGES: &str = include_str!("../data/messages.yml");

/// A chest window is 54 slots: 45 of storage and a 9 slot control row.
pub const SLOTS_PER_PAGE: usize = 45;
pub const WINDOW_SIZE: usize = 54;
pub const MAX_PAGES: i32 = 3;
pub const CHEST_CAPACITY: usize = MAX_PAGES as usize * SLOTS_PER_PAGE;
pub const BACKPACK_CAP: usize = 54;

pub fn clamp_pages(pages: i32) -> i32 {
    pages.clamp(1, MAX_PAGES)
}

pub fn capacity(pages: i32) -> usize {
    clamp_pages(pages) as usize * SLOTS_PER_PAGE
}

pub fn first_slot(page: i32) -> usize {
    (clamp_pages(page) as usize - 1) * SLOTS_PER_PAGE
}

fn clamp_rows(rows: i64) -> i32 {
    rows.clamp(1, 6) as i32
}

#[derive(Clone, Debug, PartialEq)]
pub enum Req {
    Permission(String),
    Money(f64),
    /// Minutes played.
    Playtime(i64),
}

/// One step of a ladder: the size after buying it (rows or pages) and what it costs.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub size: i32,
    pub reqs: Vec<Req>,
}

impl Step {
    pub fn money(&self) -> f64 {
        self.reqs.iter().map(|r| if let Req::Money(m) = r { *m } else { 0.0 }).sum()
    }
}

#[derive(Clone, Default)]
pub struct Settings {
    pub starting_rows: i32,
    pub max_rows: i32,
    pub item_material: String,
    pub item_texture: String,
    pub item_model_key: String,
    pub world_blacklist: Vec<String>,
    pub block_in_creative: bool,
    pub sound_open: Option<String>,
    pub sound_upgrade: Option<String>,
    /// The icons' pack: url, sha1, required, prompt (MiniMessage).
    pub pack: Option<(String, String, bool, String)>,
    pub upgrades: Vec<Step>,
    pub chest_enabled: bool,
    pub chest_starting_pages: i32,
    pub chest_max_pages: i32,
    pub chest_material: String,
    pub chest_model_key: String,
    pub chest_upgrades: Vec<Step>,
    gui: Config,
    messages: Config,
}

/// The shared `requirements:` grammar. Unknown types and amounts of 0 or less are dropped, so one typo does not
/// stop the plugin loading.
pub fn parse_reqs(list: Option<&Yaml>) -> Vec<Req> {
    let mut out = Vec::new();
    for item in list.and_then(Yaml::as_sequence).into_iter().flatten() {
        let kind = horizon_kit::yaml::string(item, "type").unwrap_or_default().to_ascii_lowercase();
        match kind.as_str() {
            "permission" => {
                let perm = horizon_kit::yaml::string(item, "permission").unwrap_or_default();
                if !perm.is_empty() {
                    out.push(Req::Permission(perm));
                }
            }
            "money" => {
                let amount = horizon_kit::yaml::num(item, "amount", 0.0);
                if amount > 0.0 {
                    out.push(Req::Money(amount));
                }
            }
            "playtime" => {
                let minutes = horizon_kit::yaml::int(item, "minutes", 0);
                if minutes > 0 {
                    out.push(Req::Playtime(minutes));
                }
            }
            _ => {}
        }
    }
    out
}

/// A ladder (`upgrades:` or `chest.upgrades:`): steps sorted by size, `key` = `rows` or `pages`.
pub fn parse_steps(list: Option<&Yaml>, key: &str, clamp: fn(i64) -> i32) -> Vec<Step> {
    let mut out: Vec<Step> = list
        .and_then(Yaml::as_sequence)
        .into_iter()
        .flatten()
        .filter(|m| m.is_mapping())
        .filter_map(|m| {
            let raw = horizon_kit::yaml::int(m, key, 0);
            (raw > 0).then(|| Step { size: clamp(raw), reqs: parse_reqs(horizon_kit::yaml::at(m, "requirements")) })
        })
        .collect();
    out.sort_by_key(|s| s.size);
    out
}

fn sound(name: String) -> Option<String> {
    let t = name.trim();
    (!t.is_empty()).then(|| t.to_ascii_uppercase())
}

impl Settings {
    pub fn load(folder: &Path) -> Settings {
        let c = Config::load(folder, "config.yml", DEFAULT_CONFIG);
        Settings::from(c, Config::load(folder, "gui.yml", DEFAULT_GUI), Config::load(folder, "messages.yml", DEFAULT_MESSAGES))
    }

    pub fn from(c: Config, gui: Config, messages: Config) -> Settings {
        let starting_rows = clamp_rows(c.int("starting-rows", 1));
        let max_rows = clamp_rows(c.int("max-rows", 6)).max(starting_rows);
        let chest_starting_pages = clamp_pages(c.int("chest.starting-pages", 1) as i32);
        let chest_max_pages = clamp_pages(c.int("chest.max-pages", MAX_PAGES as i64) as i32).max(chest_starting_pages);
        Settings {
            starting_rows,
            max_rows,
            item_material: c.str("item.material", "PLAYER_HEAD").to_ascii_uppercase(),
            item_texture: c.str("item.texture", ""),
            item_model_key: c.str("item.model-key", "zbackpacks:backpack_tier"),
            world_blacklist: c.list("world-blacklist"),
            block_in_creative: c.bool("block-in-creative", false),
            sound_open: sound(c.str("sounds.open", "")),
            sound_upgrade: sound(c.str("sounds.upgrade-success", "")),
            pack: Some(c.str("resource-pack.url", "").trim().to_owned()).filter(|u| !u.is_empty()).map(|url| {
                (url, c.str("resource-pack.sha1", "").trim().to_ascii_lowercase(), c.bool("resource-pack.required", false), c.str("resource-pack.prompt", ""))
            }),
            upgrades: parse_steps(c.at("upgrades"), "rows", clamp_rows),
            chest_enabled: c.bool("chest.enabled", true),
            chest_starting_pages,
            chest_max_pages,
            chest_material: c.str("chest.item.material", "CHEST").to_ascii_uppercase(),
            chest_model_key: c.str("chest.item.model-key", "zbackpacks:vault_chest_tier"),
            chest_upgrades: parse_steps(c.at("chest.upgrades"), "pages", |p| clamp_pages(p.clamp(0, 99) as i32)),
            gui,
            messages,
        }
    }

    /// A gui.yml text (the path itself when missing, as `GuiConfig.string`).
    pub fn gui(&self, path: &str) -> String {
        self.gui.at(path).and_then(horizon_kit::yaml::scalar).unwrap_or_else(|| path.to_owned())
    }

    pub fn gui_list(&self, path: &str) -> Vec<String> {
        self.gui.list(path)
    }

    /// The cheapest chest step above `pages`, if not maxed.
    pub fn next_chest_step(&self, pages: i32) -> Option<&Step> {
        self.chest_upgrades.iter().find(|s| s.size > pages && s.size <= self.chest_max_pages)
    }

    /// A message as a component (`<key>` filled), or None when it is blank.
    pub fn msg(&self, path: &str, pairs: &[(&str, String)]) -> Option<Value> {
        let raw = self.messages.at(path).and_then(horizon_kit::yaml::scalar)?;
        (!raw.is_empty()).then(|| mm(&raw, pairs))
    }

    /// The tier's own name (`chest.tier-names`).
    pub fn tier_name(&self, pages: i32) -> String {
        let names = self.gui_list("chest.tier-names");
        if names.is_empty() {
            return "ᴠᴀᴜʟᴛ ᴄʜᴇꜱᴛ".to_owned();
        }
        names[(pages - 1).clamp(0, names.len() as i32 - 1) as usize].clone()
    }
}

/// `<` `>` `\` escaped, so a value cannot open tags (`MiniMessageEscapes.escapePlain`).
pub fn escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 4);
    for c in input.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '<' => out.push_str("\\<"),
            '>' => out.push_str("\\>"),
            _ => out.push(c),
        }
    }
    out
}

/// `<key>` placeholders filled (Adventure's `Placeholder.unparsed`), as text that still holds MiniMessage.
pub fn fill(text: &str, pairs: &[(&str, String)]) -> String {
    let mut out = text.to_owned();
    for (k, v) in pairs {
        out = out.replace(&format!("<{k}>"), &escape(v));
    }
    out
}

/// MiniMessage, not italic (`"<!i>" + raw`), placeholders filled -> a component.
pub fn mm(text: &str, pairs: &[(&str, String)]) -> Value {
    let mut v = mini::parse(&format!("<!i>{}", fill(text, pairs)));
    if let Some(o) = v.as_object_mut()
        && !o.contains_key("italic")
    {
        o.insert("italic".into(), Value::Bool(false));
    }
    v
}

/// Lore lines: blank ones skipped, `<spacer>` an empty line.
pub fn lore_lines(lines: &[String], pairs: &[(&str, String)]) -> Vec<Value> {
    lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| if l.trim() == "<spacer>" { Value::String(String::new()) } else { mm(l, pairs) })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        Settings::from(Config::parse(DEFAULT_CONFIG), Config::parse(DEFAULT_GUI), Config::parse(DEFAULT_MESSAGES))
    }

    #[test]
    fn ladders() {
        let s = settings();
        assert_eq!((s.starting_rows, s.max_rows), (1, 6));
        assert_eq!(s.upgrades.len(), 5);
        assert_eq!(s.upgrades[1], Step { size: 3, reqs: vec![Req::Money(2500.0), Req::Playtime(60)] });
        assert_eq!(s.chest_upgrades.iter().map(|x| x.size).collect::<Vec<_>>(), vec![2, 3]);
        assert_eq!(s.next_chest_step(1).map(|x| x.size), Some(2));
        assert_eq!(s.next_chest_step(3), None);
        assert_eq!(s.sound_open.as_deref(), Some("ENTITY_ENDER_CHEST_OPEN"));
        assert_eq!(s.tier_name(2), "ɪʀᴏɴʙᴏᴜɴᴅ ᴠᴀᴜʟᴛ");
        assert_eq!(s.gui("nope.key"), "nope.key");
    }

    #[test]
    fn page_math() {
        assert_eq!(capacity(3), 135);
        assert_eq!(first_slot(2), 45);
        assert_eq!(clamp_pages(9), 3);
    }

    #[test]
    fn placeholders_are_escaped() {
        assert_eq!(fill("<white><player></white>", &[("player", "<red>x".into())]), "<white>\\<red\\>x</white>");
    }
}

