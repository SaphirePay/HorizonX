//! `MetaItemStack`: the `name:` `lore:` `sharpness:5` words after an item, written straight to the
//! stack's components.
use crate::prelude::*;
use crate::items::{can_enchant_item, enchantment_by_name, enchantment_max_level, enchantment_real_name, potion_by_name};
use crate::utils::material::{is_armor, is_banner, is_editable_book, is_firework, is_firework_charge, is_leather_armor, is_player_head, is_potion};
use crate::utils::number::decode_hex;
use serde_json::{Value, json};

const DYE_COLORS: [(&str, i64); 16] = [
    ("WHITE", 0xF9FFFE), ("ORANGE", 0xF9801D), ("MAGENTA", 0xC74EBD), ("LIGHT_BLUE", 0x3AB3DA), ("YELLOW", 0xFED83D), ("LIME", 0x80C71F), ("PINK", 0xF38BAA), ("GRAY", 0x474F52),
    ("LIGHT_GRAY", 0x9D9D97), ("CYAN", 0x169C9C), ("PURPLE", 0x8932B8), ("BLUE", 0x3C44AA), ("BROWN", 0x835432), ("GREEN", 0x5E7C16), ("RED", 0xB02E26), ("BLACK", 0x1D1D21),
];
/// `DyeColor.getFireworkColor`.
const FIREWORK_COLORS: [(&str, i64); 16] = [
    ("WHITE", 0xF0F0F0), ("ORANGE", 0xEB8844), ("MAGENTA", 0xC354CD), ("LIGHT_BLUE", 0x6689D3), ("YELLOW", 0xDECF2A), ("LIME", 0x41CD34), ("PINK", 0xD88198), ("GRAY", 0x434343),
    ("LIGHT_GRAY", 0xABABAB), ("CYAN", 0x287697), ("PURPLE", 0x7B2FBE), ("BLUE", 0x253192), ("BROWN", 0x51301A), ("GREEN", 0x3B511A), ("RED", 0xB3312C), ("BLACK", 0x1E1B1B),
];

fn is_split_char(c: char) -> bool {
    matches!(c, ':' | '+' | '\'' | ',' | ';' | '.')
}

/// `splitPattern.split(string, 2)`.
fn split_meta(text: &str) -> Vec<String> {
    match text.find(is_split_char) {
        Some(at) => {
            let width = text[at..].chars().next().map_or(1, char::len_utf8);
            vec![text[..at].to_owned(), text[at + width..].to_owned()]
        }
        None => vec![text.to_owned()],
    }
}

/// `replaceAll("(?<!\\\\)_", " ").replace("\\_", "_")`: `_` is a space, `\_` an underscore.
fn unescape_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'_') => {
                chars.next();
                out.push('_');
            }
            '_' => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

/// `split("(?<!\\\\)\\|")`, keeping `\|` for the caller.
fn split_lines(text: &str) -> Vec<String> {
    let mut lines = vec![String::new()];
    let mut previous = ' ';
    for c in text.chars() {
        if c == '|' && previous != '\\' {
            lines.push(String::new());
        } else if let Some(line) = lines.last_mut() {
            line.push(c);
        }
        previous = c;
    }
    lines
}

fn text_lines(text: &str) -> Vec<String> {
    split_lines(text).iter().map(|line| replace_format(&unescape_spaces(line).replace("\\|", "|"))).collect()
}

#[derive(Default, Clone)]
struct Firework {
    colors: Vec<i64>,
    fades: Vec<i64>,
    shape: Option<&'static str>,
    twinkle: bool,
    trail: bool,
}

impl Firework {
    fn build(&self) -> Value {
        json!({ "shape": self.shape.unwrap_or("small_ball"), "colors": self.colors, "fade_colors": self.fades, "has_trail": self.trail, "has_twinkle": self.twinkle })
    }
}

fn firework_color(text: &str) -> Option<i64> {
    let upper = text.to_uppercase();
    if let Some((_, color)) = FIREWORK_COLORS.iter().find(|(name, _)| *name == upper) {
        return Some(*color);
    }
    (text.len() == 7 && text.starts_with('#') && text[1..].bytes().all(|b| b.is_ascii_hexdigit())).then(|| decode_hex(text).map_or(0, i64::from))
}

fn shape(text: &str) -> Option<&'static str> {
    Some(match text.to_uppercase().as_str() {
        "BALL" => "small_ball",
        "BALL_LARGE" | "LARGE" => "large_ball",
        "STAR" => "star",
        "BURST" => "burst",
        "CREEPER" => "creeper",
        _ => return None,
    })
}

pub struct MetaItemStack {
    stack: ItemStack,
    builder: Firework,
    valid_firework: bool,
    valid_firework_charge: bool,
    potion_effect: Option<String>,
    valid_duration: bool,
    valid_power: bool,
    is_splash_potion: bool,
    complete_potion: bool,
    power: i32,
    duration: i32,
}

impl MetaItemStack {
    pub fn new(stack: ItemStack) -> Self {
        Self { stack, builder: Firework::default(), valid_firework: false, valid_firework_charge: false, potion_effect: None, valid_duration: false, valid_power: false, is_splash_potion: false, complete_potion: false, power: 1, duration: 120 }
    }
    pub fn get_item_stack(&self) -> &ItemStack {
        &self.stack
    }
    pub fn into_item_stack(self) -> ItemStack {
        self.stack
    }
    pub fn is_valid_firework(&self) -> bool {
        self.valid_firework
    }
    pub fn is_valid_potion(&self) -> bool {
        self.potion_effect.is_some() && self.valid_duration && self.valid_power
    }
    pub fn complete_potion(&self) -> bool {
        self.complete_potion
    }
    /// `canSpawn`.
    pub fn can_spawn(&self) -> bool {
        self.stack.get_type().is_item()
    }
    fn permission(sender: Option<&CommandSource>, meta_perm: &str, graceful: bool, include_base: bool) -> EssResult<bool> {
        let user = sender.and_then(CommandSource::get_user);
        Self::user_permission(user.as_ref(), meta_perm, graceful, include_base)
    }
    fn user_permission(user: Option<&User>, meta_perm: &str, graceful: bool, include_base: bool) -> EssResult<bool> {
        let base = if include_base { "essentials.itemspawn.meta-" } else { "essentials." };
        if user.is_none_or(|user| user.is_authorized(&format!("{base}{meta_perm}"))) {
            return Ok(true);
        }
        if graceful { Ok(false) } else { Err(EssError::tl("noMetaPerm", crate::args![meta_perm])) }
    }
    fn add_explosion(&mut self) {
        let mut fireworks = self.stack.get_component("minecraft:fireworks").cloned().unwrap_or_else(|| json!({ "flight_duration": 1, "explosions": [] }));
        if let Some(list) = fireworks["explosions"].as_array_mut() {
            list.push(self.builder.build());
        }
        self.stack.set_component("minecraft:fireworks", fireworks);
    }
    fn explosions(&self) -> usize {
        self.stack.get_component("minecraft:fireworks").and_then(|fireworks| fireworks["explosions"].as_array()).map_or(0, Vec::len)
    }
    pub fn finish_firework(&mut self, sender: Option<&CommandSource>) -> EssResult {
        fail_if!(!Self::permission(sender, "firework", true, true)?, EssError::tl("noMetaFirework", &[]));
        self.add_explosion();
        fail_if!(self.explosions() > 1 && !Self::permission(sender, "firework-multiple", true, true)?, EssError::tl("multipleCharges", &[]));
        Ok(())
    }
    /// `parseStringMeta`.
    pub fn parse_string_meta(&mut self, sender: Option<&CommandSource>, allow_unsafe: bool, words: &[String], from: usize) -> EssResult {
        let first = &words[from];
        fail_if!(first.starts_with('{') && Self::permission(sender, "vanilla", false, true)?, EssError::tl("noMetaNbtKill", &[]));
        if first.starts_with('[') && Self::permission(sender, "vanilla", false, true)? {
            self.stack = horizon_bukkit::core2::get_unsafe().modify_item_stack(&self.stack, &words[from..].join(" "));
            return Ok(());
        }
        for word in &words[from..] {
            self.add_string_meta(sender, allow_unsafe, word)?;
        }
        if self.valid_firework {
            self.finish_firework(sender)?;
        }
        if self.valid_firework_charge {
            fail_if!(!Self::permission(sender, "firework", true, true)?, EssError::tl("noMetaFirework", &[]));
            self.stack.set_component("minecraft:firework_explosion", self.builder.build());
        }
        Ok(())
    }
    /// `addStringMeta`.
    pub fn add_string_meta(&mut self, sender: Option<&CommandSource>, allow_unsafe: bool, text: &str) -> EssResult {
        let split = split_meta(text);
        let key = split[0].to_lowercase();
        let value = split.get(1);
        let material = self.stack.get_type().clone();
        if let Some(value) = value.filter(|_| key == "name")
            && Self::permission(sender, "name", false, true)?
        {
            self.stack.set_display_name(&replace_format(&unescape_spaces(value)));
        } else if let Some(value) = value.filter(|_| key == "lore" || key == "desc")
            && Self::permission(sender, "lore", false, true)?
        {
            self.stack.set_lore(text_lines(value));
        } else if (key == "custom-model-data" || key == "cmd") && Self::permission(sender, "custom-model-data", false, true)? {
            let data = match value {
                None => 0,
                Some(value) => value.parse().map_err(|_| EssError::Message(format!("For input string: \"{value}\"")))?,
            };
            self.stack.set_custom_model_data(Some(data));
        } else if key == "unbreakable" && Self::permission(sender, "unbreakable", false, true)? {
            self.stack.set_unbreakable(value.is_none_or(|value| value.eq_ignore_ascii_case("true")));
        } else if let Some(value) = value.filter(|_| key == "player" || key == "owner")
            && Self::permission(sender, "head", false, true)?
        {
            fail_if!(!is_player_head(&self.stack), EssError::tl("onlyPlayerSkulls", &[]));
            self.stack.set_component("minecraft:profile", json!({ "name": value }));
        } else if let Some(value) = value.filter(|_| key == "book" && is_editable_book(&material))
            && (Self::permission(sender, "book", true, true)? || Self::permission(sender, &format!("chapter-{}", value.to_lowercase()), true, true)?)
        {
            let input = crate::textreader::book_input("book", true).map_err(EssError::Message)?;
            if material.name() == "WRITTEN_BOOK" {
                let author = sender.and_then(CommandSource::get_player).map_or_else(crate::command_source::console_display_name, |player| player.get_name().to_owned());
                let title: String = replace_format(&value.replace('_', " ")).chars().take(32).collect();
                self.edit_book(|book| {
                    if book["author"].is_null() {
                        book["author"] = json!(author);
                    }
                    if book["title"].is_null() {
                        book["title"] = json!({ "raw": title });
                    }
                });
            }
            let pages = crate::textreader::book_pages(&input, value)?;
            self.edit_book(|book| book["pages"] = Value::Array(pages.iter().map(|page| json!({ "raw": page })).collect()));
        } else if let Some(value) = value.filter(|_| key == "author" && material.name() == "WRITTEN_BOOK")
            && Self::permission(sender, "author", false, true)?
        {
            self.edit_book(|book| book["author"] = json!(replace_format(value)));
        } else if let Some(value) = value.filter(|_| key == "title" && material.name() == "WRITTEN_BOOK")
            && Self::permission(sender, "title", false, true)?
        {
            self.edit_book(|book| book["title"] = json!({ "raw": replace_format(&unescape_spaces(value)) }));
        } else if let Some(value) = value.filter(|_| split[0].starts_with("page") && split[0].len() > 4 && is_editable_book(&material))
            && Self::permission(sender, "page", false, true)?
        {
            let number = &split[0][4..];
            let page = if is_int(number) { number.parse::<i32>().unwrap_or(1) - 1 } else { 0 };
            fail_if!(page > 100, EssError::tl("pageLimitExceeded", &[]));
            let content = text_lines(value).join("\n");
            self.edit_book(|book| {
                if !book["pages"].is_array() {
                    book["pages"] = json!([]);
                }
                if let Some(pages) = book["pages"].as_array_mut() {
                    while pages.len() <= page.max(0) as usize {
                        pages.push(json!({ "raw": "" }));
                    }
                    pages[page.max(0) as usize] = json!({ "raw": content });
                }
            });
        } else if let Some(value) = value.filter(|_| key == "power" && is_firework(&material))
            && Self::permission(sender, "firework-power", false, true)?
        {
            let power = value.parse::<i32>().unwrap_or(0);
            let mut fireworks = self.stack.get_component("minecraft:fireworks").cloned().unwrap_or_else(|| json!({ "explosions": [] }));
            fireworks["flight_duration"] = json!(if power > 3 { 4 } else { power });
            self.stack.set_component("minecraft:fireworks", fireworks);
        } else if value.is_some() && key == "itemflags" && Self::permission(sender, "itemflags", false, true)? {
            self.add_item_flags(text)?;
        } else if is_firework(&material) {
            if !self.parse_enchantment_strings(sender, allow_unsafe, &split)? {
                self.add_firework_meta(sender, false, text)?;
            }
        } else if is_firework_charge(&material) {
            if !self.parse_enchantment_strings(sender, allow_unsafe, &split)? {
                self.add_charge_meta(false, text)?;
            }
        } else if is_potion(&material) {
            if key == "power" || !self.parse_enchantment_strings(sender, allow_unsafe, &split)? {
                self.add_potion_meta(sender, false, text)?;
            }
        } else if is_banner(&material) {
            if !self.parse_enchantment_strings(sender, allow_unsafe, &split)? {
                self.add_banner_meta(text)?;
            }
        } else if let Some(value) = value.filter(|_| (key == "color" || key == "colour") && is_leather_armor(&material)) {
            let color: Vec<&str> = value.split(['|', ',']).collect();
            let rgb = match color.as_slice() {
                [input] if is_int(input) => input.parse::<i64>().unwrap_or(0),
                [input] if input.starts_with('#') => i64::from(decode_hex(input).unwrap_or(0)),
                [red, green, blue] => {
                    let part = |text: &str| text.parse::<i64>().unwrap_or(0);
                    (part(red) << 16) | (part(green) << 8) | part(blue)
                }
                _ => return Err(EssError::tl("leatherSyntax", &[])),
            };
            self.stack.set_component("minecraft:dyed_color", json!(rgb));
        } else if let Some(value) = value.filter(|_| key == "trim" && is_armor(&material)) {
            let (pattern, trim_material) = value.split_once('|').unwrap_or((value, ""));
            self.stack.set_component("minecraft:trim", json!({ "pattern": format!("minecraft:{}", pattern.to_lowercase()), "material": format!("minecraft:{}", trim_material.to_lowercase()) }));
        } else {
            self.parse_enchantment_strings(sender, allow_unsafe, &split)?;
        }
        Ok(())
    }
    fn edit_book(&mut self, edit: impl FnOnce(&mut Value)) {
        let id = if self.stack.get_type().name() == "WRITTEN_BOOK" { "minecraft:written_book_content" } else { "minecraft:writable_book_content" };
        let mut book = self.stack.get_component(id).cloned().unwrap_or_else(|| json!({}));
        edit(&mut book);
        self.stack.set_component(id, book);
    }
    /// `addItemFlags`: as far as `tooltip_display` hides what each flag stands for.
    pub fn add_item_flags(&mut self, text: &str) -> EssResult {
        let split = split_meta(text);
        let value = split.get(1).ok_or_else(|| EssError::tl("invalidItemFlagMeta", crate::args![text]))?;
        let mut hidden: Vec<Value> = self.stack.get_component("minecraft:tooltip_display").and_then(|display| display["hidden_components"].as_array().cloned()).unwrap_or_default();
        for flag in value.split(',') {
            let component = match flag.to_uppercase().as_str() {
                "HIDE_ENCHANTS" => "minecraft:enchantments",
                "HIDE_ATTRIBUTES" => "minecraft:attribute_modifiers",
                "HIDE_UNBREAKABLE" => "minecraft:unbreakable",
                "HIDE_DYE" => "minecraft:dyed_color",
                "HIDE_ARMOR_TRIM" => "minecraft:trim",
                "HIDE_STORED_ENCHANTS" => "minecraft:stored_enchantments",
                "HIDE_DESTROYS" => "minecraft:can_break",
                "HIDE_PLACED_ON" => "minecraft:can_place_on",
                _ => continue,
            };
            hidden.push(json!(component));
        }
        fail_if!(hidden.is_empty(), EssError::tl("invalidItemFlagMeta", crate::args![text]));
        self.stack.set_component("minecraft:tooltip_display", json!({ "hidden_components": hidden }));
        Ok(())
    }
    fn parse_colors(&self, list: &str, key: &str) -> EssResult<Vec<i64>> {
        list.split(',').map(|color| firework_color(color).ok_or_else(|| EssError::tl("invalidFireworkFormat", crate::args![list, key]))).collect()
    }
    fn parse_shared(&mut self, split: &[String], allow_short: bool, key: &str) -> EssResult {
        let value = &split[1];
        if key == "shape" || key == "type" || (allow_short && (key == "s" || key == "t")) {
            self.builder.shape = Some(shape(value).ok_or_else(|| EssError::tl("invalidFireworkFormat", crate::args![value, split[0].clone()]))?);
        } else if key == "fade" || (allow_short && key == "f") {
            let fades = self.parse_colors(value, &split[0])?;
            self.builder.fades.extend(fades);
        } else if key == "effect" || (allow_short && key == "e") {
            for effect in value.split(',') {
                match effect.to_lowercase().as_str() {
                    "twinkle" => self.builder.twinkle = true,
                    "trail" => self.builder.trail = true,
                    _ => return Err(EssError::tl("invalidFireworkFormat", crate::args![value, split[0].clone()])),
                }
            }
        }
        Ok(())
    }
    fn add_charge_meta(&mut self, allow_short: bool, text: &str) -> EssResult {
        let split = split_meta(text);
        if split.len() < 2 {
            return Ok(());
        }
        let key = split[0].to_lowercase();
        if key == "color" || key == "colour" || (allow_short && key == "c") {
            let colors = self.parse_colors(&split[1], &split[0])?;
            self.valid_firework_charge = true;
            self.builder.colors.extend(colors);
            return Ok(());
        }
        self.parse_shared(&split, allow_short, &key)
    }
    /// `addFireworkMeta`.
    pub fn add_firework_meta(&mut self, sender: Option<&CommandSource>, allow_short: bool, text: &str) -> EssResult {
        if !is_firework(self.stack.get_type()) {
            return Ok(());
        }
        let split = split_meta(text);
        if split.len() < 2 {
            return Ok(());
        }
        let key = split[0].to_lowercase();
        if key == "color" || key == "colour" || (allow_short && key == "c") {
            if self.valid_firework {
                self.finish_firework(sender)?;
                self.builder = Firework::default();
            }
            let colors = self.parse_colors(&split[1], &split[0])?;
            self.valid_firework = true;
            self.builder.colors.extend(colors);
            return Ok(());
        }
        self.parse_shared(&split, allow_short, &key)
    }
    /// `addPotionMeta`.
    pub fn add_potion_meta(&mut self, sender: Option<&CommandSource>, allow_short: bool, text: &str) -> EssResult {
        if !is_potion(self.stack.get_type()) {
            return Ok(());
        }
        let split = split_meta(text);
        if split.len() < 2 {
            return Ok(());
        }
        let (key, value) = (split[0].to_lowercase(), &split[1]);
        let invalid = || EssError::tl("invalidPotionMeta", crate::args![value]);
        if key == "effect" || (allow_short && key == "e") {
            let effect = potion_by_name(value).ok_or_else(invalid)?;
            fail_if!(!Self::permission(sender, &format!("potions.{effect}"), true, false)?, EssError::tl("noPotionEffectPerm", crate::args![effect]));
            self.potion_effect = Some(effect);
        } else if key == "power" || (allow_short && key == "p") {
            self.power = value.parse().map_err(|_| invalid())?;
            self.valid_power = true;
            if self.power > 0 && self.power < 4 {
                self.power -= 1;
            }
        } else if key == "amplifier" || (allow_short && key == "a") {
            self.power = value.parse().map_err(|_| invalid())?;
            self.valid_power = true;
        } else if key == "duration" || (allow_short && key == "d") {
            let parsed: i32 = value.parse().map_err(|_| invalid())?;
            self.valid_duration = true;
            self.duration = if parsed == -1 { -1 } else { parsed.saturating_mul(20) };
        } else if key == "splash" || (allow_short && key == "s") {
            self.is_splash_potion = value.eq_ignore_ascii_case("true");
        }
        if self.is_valid_potion() {
            let mut contents = self.stack.get_component("minecraft:potion_contents").cloned().unwrap_or_else(|| json!({}));
            if !contents.is_object() {
                contents = json!({ "potion": contents });
            }
            if !contents["custom_effects"].is_array() {
                contents["custom_effects"] = json!([]);
            }
            fail_if!(contents["custom_effects"].as_array().map_or(0, Vec::len) > 1 && !Self::permission(sender, "potions.multiple", true, false)?, EssError::tl("multiplePotionEffects", &[]));
            let id = format!("minecraft:{}", self.potion_effect.clone().unwrap_or_default());
            if let Some(list) = contents["custom_effects"].as_array_mut() {
                list.retain(|effect| effect["id"].as_str() != Some(&id));
                list.push(json!({ "id": id, "amplifier": self.power, "duration": self.duration }));
            }
            self.stack.set_component("minecraft:potion_contents", contents);
            if self.is_splash_potion && self.stack.get_type().name() == "POTION" {
                self.stack.set_type(horizon_bukkit::Material::of("SPLASH_POTION"));
            }
            self.potion_effect = None;
            self.valid_duration = false;
            self.valid_power = false;
            self.is_splash_potion = false;
            self.complete_potion = true;
        }
        Ok(())
    }
    fn parse_enchantment_strings(&mut self, sender: Option<&CommandSource>, allow_unsafe: bool, split: &[String]) -> EssResult<bool> {
        let Some(enchantment) = enchantment_by_name(&split[0]) else { return Ok(false) };
        if Self::permission(sender, &format!("enchantments.{}", enchantment_real_name(&enchantment)), false, false)? {
            let max = enchantment_max_level(&enchantment);
            let mut level = split.get(1).and_then(|text| text.parse::<i32>().ok()).unwrap_or(-1);
            if level < 0 || (!allow_unsafe && level > max) {
                level = max;
            }
            self.add_enchantment(allow_unsafe, &enchantment, level)?;
        }
        Ok(true)
    }
    /// `addEnchantment`.
    pub fn add_enchantment(&mut self, allow_unsafe: bool, enchantment: &str, level: i32) -> EssResult {
        if self.stack.get_type().name() == "ENCHANTED_BOOK" {
            let key = format!("minecraft:{}", enchantment_real_name(enchantment));
            let mut stored = self.stack.get_component("minecraft:stored_enchantments").cloned().unwrap_or_else(|| json!({}));
            if level == 0 {
                if let Some(levels) = stored.as_object_mut() {
                    levels.remove(&key);
                }
            } else {
                stored[key] = json!(level);
            }
            self.stack.set_component("minecraft:stored_enchantments", stored);
        } else if level == 0 {
            self.stack.remove_enchantment(enchantment);
        } else {
            if !allow_unsafe {
                // `ItemStack.addEnchantment`'s two checks, under `addEnchantment`'s wrapper.
                let name = enchantment_real_name(enchantment);
                let max = enchantment_max_level(enchantment);
                fail_if!(level < 1 || level > max, EssError::Message(format!("Enchantment {name}: Enchantment level is either too low or too high (given {level}, bounds are 1 to {max})")));
                fail_if!(!can_enchant_item(enchantment, self.stack.get_type()), EssError::Message(format!("Enchantment {name}: Specified enchantment cannot be applied to this itemstack")));
            }
            self.stack.add_enchant(enchantment, level as u32);
        }
        Ok(())
    }
    /// `getEnchantment`.
    pub fn get_enchantment(&self, user: Option<&User>, name: &str) -> EssResult<Option<String>> {
        let Some(enchantment) = enchantment_by_name(name) else { return Ok(None) };
        fail_if!(!Self::user_permission(user, &format!("enchantments.{enchantment}"), true, false)?, EssError::tl("enchantmentPerm", crate::args![enchantment]));
        Ok(Some(enchantment))
    }
    /// `addBannerMeta`.
    pub fn add_banner_meta(&mut self, text: &str) -> EssResult {
        let split = split_meta(text);
        let value = split.get(1).ok_or_else(|| EssError::tl("invalidBanner", crate::args![text]))?;
        let dye = |rgb: &str| -> EssResult<&'static str> {
            let rgb: i64 = rgb.parse().map_err(|_| EssError::Message(format!("For input string: \"{rgb}\"")))?;
            Ok(DYE_COLORS.iter().find(|(_, known)| *known == rgb).map_or("WHITE", |(name, _)| *name))
        };
        if split[0].eq_ignore_ascii_case("basecolor") {
            let color = dye(value)?.to_lowercase();
            if self.stack.get_type().name() == "SHIELD" {
                self.stack.set_component("minecraft:base_color", json!(color));
            } else {
                self.stack.set_type(horizon_bukkit::Material::of(&format!("{color}_banner")));
            }
        } else {
            let mut patterns = self.stack.get_component("minecraft:banner_patterns").cloned().unwrap_or_else(|| json!([]));
            if let Some(list) = patterns.as_array_mut() {
                list.push(json!({ "pattern": format!("minecraft:{}", split[0].to_lowercase()), "color": dye(value)?.to_lowercase() }));
            }
            self.stack.set_component("minecraft:banner_patterns", patterns);
        }
        let _ = ess();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_is_the_first_mark_only() {
        assert_eq!(split_meta("name:a:b"), ["name", "a:b"]);
        assert_eq!(split_meta("sharpness"), ["sharpness"]);
    }

    #[test]
    fn underscores_are_spaces_unless_escaped() {
        assert_eq!(unescape_spaces("a_b\\_c"), "a b_c");
        assert_eq!(split_lines("a|b\\|c"), ["a", "b\\|c"]);
        assert_eq!(text_lines("a_b|c\\|d"), ["a b", "c|d"]);
    }
}
