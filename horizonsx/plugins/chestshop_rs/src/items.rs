//! Item names on signs (`MaterialUtil`): `Diamond Sword`, shortened to fit a sign line (`DiamondSwo`),
//! `:damage`, `#code` for an item with data (the item database), and parsing them back; item aliases.

use horizon_bukkit::{ItemStack, Material};
use serde_json::{Map, Value, json};
use std::collections::HashMap;

const CHARS: &str = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_'abcdefghijklmnopqrstuvwxyz{|}~¦ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜø£Ø×\u{83}áíóúñÑªº¿®¬½¼¡«»";
const WIDTHS: [u8; 224] = [
    4, 2, 5, 6, 6, 6, 6, 3, 5, 5, 5, 6, 2, 6, 2, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 2, 2, 5, 6, 5, 6, 7, 6, 6, 6, 6, 6, 6, 6, 6, 4, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 4, 6, 4, 6, 6, 3, 6, 6, 6,
    6, 6, 5, 6, 6, 2, 6, 5, 3, 6, 6, 6, 6, 6, 6, 6, 4, 6, 6, 6, 6, 6, 6, 5, 2, 5, 7, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 4, 6, 3, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 4, 6, 6, 3, 6, 6, 6, 6, 6, 6,
    6, 7, 6, 6, 6, 2, 6, 6, 8, 9, 9, 6, 6, 6, 8, 8, 6, 8, 8, 8, 8, 8, 6, 6, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 6, 9, 9, 9, 5, 9, 9, 8, 7, 7, 8, 7, 8, 8, 8, 7, 8, 8, 7, 9, 9,
    6, 7, 7, 7, 7, 7, 9, 6, 7, 8, 7, 6, 6, 9, 7, 6, 7, 1,
];

/// `StringUtil.getMinecraftCharWidth`.
pub fn char_width(c: char) -> i32 {
    if c == '§' {
        return 0;
    }
    CHARS.chars().position(|x| x == c).map_or(10, |i| i32::from(WIDTHS[i]))
}

pub fn width(s: &str) -> i32 {
    s.chars().map(char_width).sum()
}

/// The widest an item code may be on a sign: "---------------".
pub fn max_sign_width() -> i32 {
    width("---------------")
}

/// `capitalizeFirstLetter(name, sep)`: `DIAMOND_SWORD` -> `Diamond Sword`.
pub fn capitalize(s: &str, sep: char) -> String {
    s.to_lowercase()
        .split(sep)
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `getShortenedName`.
pub fn shortened(name: &str, max: i32) -> String {
    let name = capitalize(&name.replace('_', " "), ' ');
    if width(&name) <= max {
        return name;
    }
    let mut parts: Vec<String> = name.split([' ', '-']).map(str::to_owned).collect();
    let joined = parts.concat();
    if width(&joined) <= max {
        return joined;
    }
    let mut exceeding = width(&joined) - max;
    let (mut shortest, mut longest) = (0, 0);
    for i in 0..parts.len() {
        if width(&parts[longest]) < width(&parts[i]) {
            longest = i;
        }
        if width(&parts[shortest]) > width(&parts[i]) {
            shortest = i;
        }
    }
    let shortest_w = width(&parts[shortest]);
    let mut remove = width(&parts[longest]) - shortest_w;
    while remove > 0 && exceeding > 0 && !parts[longest].is_empty() {
        let end = parts[longest].pop().map_or(0, char_width);
        remove -= end;
        exceeding -= end;
    }
    for i in (0..parts.len()).rev() {
        if exceeding <= 0 {
            break;
        }
        let w = width(&parts[i]);
        if w > shortest_w {
            remove = w - shortest_w;
        }
        if remove > exceeding {
            remove = exceeding;
        }
        while remove > 0 && !parts[i].is_empty() {
            let end = parts[i].pop().map_or(0, char_width);
            remove -= end;
            exceeding -= end;
        }
    }
    while exceeding > 0 && parts.iter().any(|p| !p.is_empty()) {
        for i in (0..parts.len()).rev() {
            if exceeding <= 0 {
                break;
            }
            if let Some(c) = parts[i].pop() {
                exceeding -= char_width(c);
            }
        }
    }
    parts.concat()
}

/// Every Bukkit material name (no legacy ones).
fn materials() -> &'static [&'static str] {
    horizon_bukkit::enums::material::values()
}

/// `(?<!^)([A-Z1-9])` -> `_$1`.
fn underscore_caps(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if i > 0 && (c.is_ascii_uppercase() || ('1'..='9').contains(&c)) {
            out.push('_');
        }
        out.push(c);
    }
    out
}

/// `MaterialUtil.getMaterial`: `Material.matchMaterial`, else the enum parser's prefix matching.
pub fn material(name: &str) -> Option<String> {
    let n = name.trim();
    if n.is_empty() {
        return None;
    }
    let matched = n.trim_start_matches("minecraft:").replace(' ', "_").to_ascii_uppercase();
    let valid = |m: &str| !m.starts_with("LEGACY_") && materials().contains(&m);
    if valid(&matched) {
        return Some(matched);
    }
    let up = n.to_ascii_uppercase();
    if valid(&up) {
        return Some(up);
    }
    let type_parts: Vec<String> = underscore_caps(n).to_ascii_uppercase().split([' ', '_']).map(str::to_owned).collect();
    let mut current: Option<&str> = None;
    let mut length = usize::MAX;
    for m in materials().iter().filter(|m| !m.starts_with("LEGACY_")) {
        if m.len() < length && m.starts_with(n) {
            length = m.len();
            current = Some(m);
        } else if type_parts.len() > 1 {
            let name_parts: Vec<&str> = m.split('_').collect();
            if name_parts.len() == type_parts.len() && name_parts.iter().zip(&type_parts).all(|(a, b)| a.starts_with(b.as_str())) {
                current = Some(m);
                break;
            }
        }
    }
    current.map(str::to_owned)
}

/// What a stack carries beyond its type, as stored in the item database (count 1, no damage).
fn data_of(stack: &ItemStack) -> Map<String, Value> {
    stack.all_components().filter(|(k, _)| k.as_str() != "minecraft:damage").map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// The item database (`items.db` in Java): code -> an item's data, written to itemdb.json.
#[derive(Default, Clone)]
pub struct ItemDb {
    pub next: u64,
    pub items: HashMap<u64, (String, Map<String, Value>)>,
    pub by_key: HashMap<String, u64>,
    pub dirty: bool,
}

const BASE62: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

pub fn base62(mut n: u64) -> String {
    if n == 0 {
        return "0".into();
    }
    let mut out = Vec::new();
    while n > 0 {
        out.push(BASE62[(n % 62) as usize]);
        n /= 62;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

pub fn unbase62(code: &str) -> Option<u64> {
    code.bytes().try_fold(0u64, |acc, b| BASE62.iter().position(|x| *x == b).map(|i| acc * 62 + i as u64))
}

impl ItemDb {
    pub fn from_json(v: &Value) -> ItemDb {
        let mut db = ItemDb { next: v["next"].as_u64().unwrap_or(0), ..ItemDb::default() };
        for (k, item) in v["items"].as_object().into_iter().flatten() {
            let Ok(id) = k.parse::<u64>() else { continue };
            let material = item["id"].as_str().unwrap_or("minecraft:stone").to_owned();
            let data = item["components"].as_object().cloned().unwrap_or_default();
            db.by_key.insert(key(&material, &data), id);
            db.items.insert(id, (material, data));
            db.next = db.next.max(id);
        }
        db
    }
    pub fn to_json(&self) -> Value {
        let items: Map<String, Value> = self.items.iter().map(|(id, (m, d))| (id.to_string(), json!({ "id": m, "components": d }))).collect();
        json!({ "next": self.next, "items": items })
    }
    /// `getItemCode`: the stack's data under a code (made the first time).
    pub fn code(&mut self, stack: &ItemStack) -> String {
        let material = stack.get_type().key().to_owned();
        let data = data_of(stack);
        let k = key(&material, &data);
        if let Some(id) = self.by_key.get(&k) {
            return base62(*id);
        }
        self.next += 1;
        let id = self.next;
        self.by_key.insert(k, id);
        self.items.insert(id, (material, data));
        self.dirty = true;
        base62(id)
    }
    pub fn get(&self, code: &str) -> Option<&Map<String, Value>> {
        self.items.get(&unbase62(code)?).map(|(_, d)| d)
    }
}

fn key(material: &str, data: &Map<String, Value>) -> String {
    format!("{material}|{}", Value::Object(data.clone()))
}

/// `getName(item, maxWidth)`: the sign/list name of a stack.
pub fn name(db: &mut ItemDb, stack: &ItemStack, max: i32) -> String {
    let damage = stack.get_damage();
    let durability = if damage != 0 { format!(":{damage}") } else { String::new() };
    let meta = if data_of(stack).is_empty() { String::new() } else { format!("#{}", db.code(stack)) };
    let mut code = capitalize(&stack.get_type().name(), '_');
    if max > 0 {
        let w = width(&format!("{code}{durability}{meta}"));
        if w > max {
            code = shortened(&code, width(&code) - (w - max));
        }
    }
    format!("{code}{durability}{meta}")
}

/// `MaterialUtil.getItem`: a stack (one) from a sign's item code.
pub fn parse(db: &ItemDb, code: &str) -> Option<ItemStack> {
    let first = code.split([':', '-', '#']).next().unwrap_or_default().trim();
    let material = material(first)?;
    let mut stack = ItemStack::new(Material::of(&material), 1);
    if let Some(start) = code.find('#') {
        let meta: String = code[start + 1..].chars().take_while(char::is_ascii_alphanumeric).collect();
        if let Some(data) = db.get(&meta) {
            for (k, v) in data {
                stack.set_component(k, v.clone());
            }
        }
    }
    if let Some(start) = code.find(':') {
        let digits: String = code[start + 1..].chars().take_while(char::is_ascii_digit).collect();
        if let Ok(d) = digits.parse::<i16>()
            && d > 0
        {
            stack.set_damage(i32::from(d));
        }
    }
    Some(stack)
}

/// Item aliases (itemAliases.yml: alias -> item code).
#[derive(Default, Clone)]
pub struct Aliases(pub Vec<(String, String)>);

impl Aliases {
    /// `onItemParse`: an alias (or the start of one) to its code.
    pub fn resolve(&self, typed: &str) -> Option<String> {
        if let Some((k, _)) = self.0.iter().find(|(_, v)| v == typed) {
            return Some(k.clone());
        }
        let parts: Vec<String> = underscore_caps(typed).to_ascii_uppercase().split([' ', '_', '-']).map(str::to_owned).collect();
        let mut code = None;
        let mut length = usize::MAX;
        for (k, v) in &self.0 {
            if v.len() < length && v.to_ascii_uppercase().starts_with(&typed.to_ascii_uppercase()) {
                length = v.len();
                code = Some(k.clone());
            } else if parts.len() > 1 {
                let name_parts: Vec<String> = v.to_ascii_uppercase().split([' ', '_', '-']).map(str::to_owned).collect();
                if name_parts.len() == parts.len() && name_parts.iter().zip(&parts).all(|(a, b)| a.starts_with(b.as_str())) {
                    return Some(k.clone());
                }
            }
        }
        code
    }
    /// `onItemStringQuery`: a code shown as its alias.
    pub fn alias_of(&self, code: &str) -> Option<String> {
        self.0.iter().find(|(k, _)| k == code || k.eq_ignore_ascii_case(code)).map(|(_, v)| v.clone())
    }
}

/// Parses an item code or alias.
pub fn parse_any(db: &ItemDb, aliases: &Aliases, code: &str) -> Option<ItemStack> {
    if let Some(real) = aliases.resolve(code) {
        return parse(db, &real);
    }
    parse(db, code)
}

/// `MaterialUtil.equals`: the same item (type and data; amounts aside).
pub fn same(a: &ItemStack, b: &ItemStack) -> bool {
    a.get_type() == b.get_type() && a.all_components().collect::<Vec<_>>() == b.all_components().collect::<Vec<_>>()
}

#[cfg(test)]
mod tests {
    #[test]
    fn names() {
        assert_eq!(super::capitalize("DIAMOND_SWORD", '_'), "Diamond Sword");
        assert_eq!(super::base62(61), "Z");
        assert_eq!(super::unbase62("10"), Some(62));
        let s = super::shortened("Light Blue Stained Glass Pane", super::max_sign_width());
        assert!(super::width(&s) <= super::max_sign_width(), "{s}");
    }
}
