//! `FlatItemDb` over `items.json`, `CustomItemResolver` over `custom_items.yml`, and the serialize half
//! of `AbstractItemDb`. `items.csv` is the pre-1.13 database: nothing reads it on a modern game.
use crate::prelude::*;
use crate::config::{Config, scalar_string};
use crate::items::enchantment_real_name;
use crate::utils::format::unformat_string;
use crate::utils::material::{is_firework, is_firework_charge, is_leather_armor, is_player_head, is_potion};
use serde_json::Value;
use serde_json::value::RawValue;
use std::collections::BTreeMap;
use std::sync::{Mutex, RwLock};

#[derive(Clone, Debug, PartialEq, Eq)]
struct PotionData {
    kind: String,
    upgraded: bool,
    extended: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ItemData {
    material: String,
    potion: Option<PotionData>,
    entity: Option<String>,
}

/// The file has about 50,000 names for 2,000 items, so each name is kept once, in sorted order.
#[derive(Default)]
struct Data {
    /// Every name, with its place in `items`: `None` for an alias of something the file doesn't have.
    names: Vec<(Box<str>, Option<u32>)>,
    /// What has an entry of its own, by name: `name` takes the first that matches.
    items: Vec<(Box<str>, ItemData)>,
}

impl Data {
    fn item_place(&self, name: &str) -> Option<u32> {
        self.items.binary_search_by(|(known, _)| known.as_ref().cmp(name)).ok().map(|place| place as u32)
    }
    fn item(&self, name: &str) -> Option<&ItemData> {
        let place = self.names.binary_search_by(|(known, _)| known.as_ref().cmp(name)).ok()?;
        self.names[place].1.map(|item| &self.items[item as usize].1)
    }
    fn contains(&self, name: &str) -> bool {
        self.names.binary_search_by(|(known, _)| known.as_ref().cmp(name)).is_ok()
    }
}

pub struct ItemDb {
    data: RwLock<Data>,
    /// `CustomItemResolver`'s map.
    custom: Mutex<BTreeMap<String, String>>,
    custom_config: Mutex<Config>,
}

fn file_text() -> String {
    let path = crate::data_folder().join("items.json");
    // `ManagedFile`: the shipped file is written out, and kept up to date with the plugin.
    let shipped = include_str!("../../resources/items.json");
    match std::fs::read_to_string(&path) {
        Ok(text) if !text.trim().is_empty() => text,
        _ => {
            let _ = std::fs::write(&path, shipped);
            shipped.to_owned()
        }
    }
}

fn read_data(text: &str) -> Data {
    let mut data = Data::default();
    let json: String = text.lines().filter(|line| !line.starts_with('#')).collect();
    // Raw values: a tree of the whole file is 13 MB that the heap keeps after it's gone.
    let Ok(map) = serde_json::from_str::<BTreeMap<String, &RawValue>>(&json) else {
        crate::severe("items.json could not be read");
        return data;
    };
    for (key, raw) in map.iter().filter(|(_, raw)| raw.get().starts_with('{')) {
        if let Ok(Value::Object(fields)) = serde_json::from_str(raw.get()) {
            let material = fields.get("material").and_then(Value::as_str).or_else(|| fields.get("fallbacks").and_then(|list| list[0].as_str())).unwrap_or("AIR").to_owned();
            let potion = fields.get("potionData").map(|potion| PotionData {
                kind: potion["type"].as_str().or(potion["fallbackType"].as_str()).unwrap_or_default().to_owned(),
                upgraded: potion["upgraded"].as_bool().unwrap_or(false),
                extended: potion["extended"].as_bool().unwrap_or(false),
            });
            let entity = fields.get("entity").and_then(Value::as_str).map(str::to_owned);
            data.items.push((key.as_str().into(), ItemData { material, potion, entity }));
        }
    }
    data.names.reserve_exact(map.len());
    for (key, raw) in map {
        let item = match raw.get().as_bytes()[0] {
            b'{' => data.item_place(&key),
            b'"' => serde_json::from_str::<String>(raw.get()).ok().and_then(|target| data.item_place(&target)),
            _ => {
                crate::warning(&format!("Failed to add item: \"{key}\": {}", raw.get()));
                continue;
            }
        };
        data.names.push((key.into_boxed_str(), item));
    }
    data
}

/// `PotionType`, named as the game has it for a base potion.
fn base_potion_id(potion: &PotionData) -> String {
    let name = potion.kind.to_lowercase();
    if potion.upgraded {
        format!("minecraft:strong_{name}")
    } else if potion.extended {
        format!("minecraft:long_{name}")
    } else {
        format!("minecraft:{name}")
    }
}

impl ItemDb {
    pub fn new() -> Self {
        let config = Config::with_template(crate::data_folder().join("custom_items.yml"), include_str!("../../resources/custom_items.yml"));
        Self { data: RwLock::new(Data::default()), custom: Mutex::new(BTreeMap::new()), custom_config: Mutex::new(config) }
    }
    fn data(&self) -> std::sync::RwLockReadGuard<'_, Data> {
        self.data.read().unwrap_or_else(|e| e.into_inner())
    }
    /// `reloadConfig`, of the database and then of `CustomItemResolver`.
    pub fn reload_config(&self) {
        *self.data.write().unwrap_or_else(|e| e.into_inner()) = read_data(&file_text());
        let extra = self.custom.lock().unwrap_or_else(|e| e.into_inner()).keys().filter(|name| !self.data().contains(name)).count();
        crate::info(&format!("Loaded {} items from items.json.", self.data().names.len() + extra));
        let mut config = self.custom_config.lock().unwrap_or_else(|e| e.into_inner());
        config.load();
        let section = config.section_map("aliases");
        let mut custom = BTreeMap::new();
        if section.is_empty() {
            crate::warning("No aliases found in custom_items.yml.");
        }
        for (alias, target) in section {
            if !target.is_string() {
                continue;
            }
            if let Some(target) = scalar_string(&target)
                && self.get_flat(&target).is_ok()
            {
                custom.insert(alias.to_lowercase(), target);
            }
        }
        *self.custom.lock().unwrap_or_else(|e| e.into_inner()) = custom;
    }
    /// `isReady`.
    pub fn is_ready(&self) -> bool {
        !self.data().items.is_empty()
    }
    fn by_name(&self, name: &str) -> Option<ItemData> {
        self.data().item(&name.to_lowercase()).cloned()
    }
    fn get_flat(&self, id: &str) -> EssResult<ItemStack> {
        let id = id.to_lowercase();
        // `split("(?<!^minecraft):")`: the id's own colon doesn't count.
        let (name, damage) = match id.strip_prefix("minecraft:") {
            Some(rest) => match rest.split_once(':') {
                Some((name, damage)) => (format!("minecraft:{name}"), Some(damage.to_owned())),
                None => (id.clone(), None),
            },
            None => match id.split_once(':') {
                Some((name, damage)) => (name.to_owned(), Some(damage.to_owned())),
                None => (id.clone(), None),
            },
        };
        let data = self.by_name(&name).ok_or_else(|| EssError::tl("unknownItemName", crate::args![id.clone()]))?;
        let material = Material::of(&data.material);
        fail_if!(!material.is_item(), EssError::tl("unableToSpawnItem", crate::args![id]));
        let mut stack = ItemStack::new(material.clone(), material.get_max_stack_size());
        if let Some(potion) = &data.potion {
            stack.set_component("minecraft:potion_contents", serde_json::json!({ "potion": base_potion_id(potion) }));
        }
        if let Some(damage) = damage {
            stack.set_damage(damage.parse::<i32>().map_err(|_| EssError::Message(format!("For input string: \"{damage}\"")))?);
        }
        if let Some(entity) = data.entity.as_ref().filter(|_| material.name().contains("SPAWNER")) {
            stack.set_component("minecraft:block_entity_data", serde_json::json!({ "id": "minecraft:mob_spawner", "SpawnData": { "entity": { "id": format!("minecraft:{}", entity.to_lowercase()) } } }));
            stack.set_component("minecraft:custom_data", serde_json::json!({ "convert": "true" }));
        }
        Ok(stack)
    }
    /// `get(id)`: a custom alias first, then the database.
    pub fn get(&self, id: &str) -> EssResult<ItemStack> {
        let target = self.custom.lock().unwrap_or_else(|e| e.into_inner()).get(&id.to_lowercase()).cloned();
        if let Some(target) = target
            && let Ok(stack) = self.get_flat(&target)
        {
            return Ok(stack);
        }
        self.get_flat(id)
    }
    /// `get(id, amount)`.
    pub fn get_amount(&self, id: &str, amount: i32) -> EssResult<ItemStack> {
        let mut stack = self.get(id)?;
        stack.set_amount(amount);
        Ok(stack)
    }
    /// `name`: the primary name of a stack.
    pub fn name(&self, item: &ItemStack) -> Option<String> {
        let material = item.get_type().name();
        let data = self.data();
        let potion = item.get_component("minecraft:potion_contents").and_then(|contents| contents["potion"].as_str().or(contents.as_str())).map(str::to_owned);
        data.items
            .iter()
            .find(|(_, known)| {
                if !known.material.eq_ignore_ascii_case(&material) {
                    return false;
                }
                match (&known.potion, &potion) {
                    (Some(known), Some(held)) => base_potion_id(known) == *held || format!("minecraft:{held}") == base_potion_id(known),
                    (None, _) => true,
                    (Some(_), None) => false,
                }
            })
            .map(|(name, _)| name.to_string())
    }
    /// `nameList`.
    pub fn name_list(&self, item: &ItemStack) -> Vec<String> {
        let Some(primary) = self.name(item) else { return vec![String::new()] };
        let data = self.data();
        let item = data.item_place(&primary);
        let mut names = vec![primary.clone()];
        names.extend(data.names.iter().filter(|(name, place)| *place == item && name.as_ref() != primary).map(|(name, _)| name.to_string()));
        names
    }
    /// `listNames`, of those that can still become what's been typed: tab completion filters by that
    /// anyway, and this runs on a key press, so the rest aren't copied first.
    pub fn list_names(&self, typed: &str) -> Vec<String> {
        let fits = |name: &str| name.get(..typed.len()).is_some_and(|start| start.eq_ignore_ascii_case(typed));
        let data = self.data();
        let custom = self.custom.lock().unwrap_or_else(|e| e.into_inner());
        let mut names: Vec<String> = data.names.iter().map(|(name, _)| name.as_ref()).chain(custom.keys().map(String::as_str)).filter(|name| fits(name)).map(str::to_owned).collect();
        names.sort_unstable();
        names.dedup();
        names
    }
    /// `CustomItemResolver.getAliasesFor`.
    pub fn get_aliases_for(&self, item: &str) -> Vec<String> {
        let custom = self.custom.lock().unwrap_or_else(|e| e.into_inner()).clone();
        custom.iter().filter(|(_, target)| self.get(target).ok().and_then(|stack| self.name(&stack)).is_some_and(|name| name.eq_ignore_ascii_case(item))).map(|(alias, _)| alias.clone()).collect()
    }
    fn save_custom(&self) {
        let custom = self.custom.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut config = self.custom_config.lock().unwrap_or_else(|e| e.into_inner());
        config.set("aliases", serde_yaml::Value::Mapping(custom.into_iter().map(|(alias, target)| (alias.into(), target.into())).collect()));
        config.save();
    }
    /// `CustomItemResolver.setAlias`.
    pub fn set_alias(&self, alias: &str, target: &str) {
        let alias = alias.to_lowercase();
        {
            let mut custom = self.custom.lock().unwrap_or_else(|e| e.into_inner());
            if custom.get(&alias).is_some_and(|known| known.eq_ignore_ascii_case(target)) {
                return;
            }
            custom.insert(alias, target.to_owned());
        }
        self.save_custom();
    }
    /// `CustomItemResolver.removeAlias`.
    pub fn remove_alias(&self, alias: &str) {
        if self.custom.lock().unwrap_or_else(|e| e.into_inner()).remove(&alias.to_lowercase()).is_some() {
            self.save_custom();
        }
    }
    /// `getMatching`.
    pub fn get_matching(&self, user: &User, args: &[&str]) -> EssResult<Vec<ItemStack>> {
        let held = || user.get_item_in_hand().unwrap_or_else(ItemStack::empty);
        let every = |blocks_only: bool| -> Vec<ItemStack> {
            let Some(player) = user.player() else { return Vec::new() };
            inventories::get_inventory(&player, true).into_iter().flatten().filter(|stack| !stack.get_type().is_air() && (!blocks_only || stack.get_type().is_block())).collect()
        };
        let list = match args.first().map(|arg| arg.to_lowercase()).as_deref() {
            None | Some("hand") => vec![held()],
            Some("inventory" | "invent" | "all") => every(false),
            Some("blocks") => every(true),
            Some(_) => vec![self.get(args[0])?],
        };
        fail_if!(list.first().is_none_or(|first| first.get_type().is_air()), EssError::Message(user.player_tl("itemSellAir", &[])));
        Ok(list)
    }
    /// `serialize`: the line `/createkit` and such write.
    pub fn serialize(&self, stack: &ItemStack) -> String {
        let mut text = format!("{} {} ", self.name(stack).unwrap_or_default(), stack.get_amount());
        if stack.has_item_meta() {
            if let Some(name) = stack.get_display_name() {
                text.push_str(&format!("name:{} ", unformat_string(&name).replace(' ', "_")));
            }
            if stack.has_lore() {
                text.push_str(&format!("lore:{} ", serialize_lines(&stack.get_lore())));
            }
            if let Some(data) = stack.get_custom_model_data() {
                text.push_str(&format!("custom-model-data:{data} "));
            }
            for (enchantment, level) in stack.get_enchants() {
                text.push_str(&format!("{}:{level} ", enchantment_real_name(&enchantment)));
            }
        }
        let material = stack.get_type();
        if is_firework(material) || is_firework_charge(material) {
            if settings().is_debug() {
                crate::info("firework meta isn't written by serialize yet");
            }
        } else if is_potion(material) {
            let splash = material.name() == "SPLASH_POTION";
            for effect in stack.get_component("minecraft:potion_contents").and_then(|contents| contents["custom_effects"].as_array()).into_iter().flatten() {
                text.push_str(&format!("splash:{splash} effect:{} power:{} duration:{} ", effect["id"].as_str().unwrap_or_default().trim_start_matches("minecraft:"), effect["amplifier"].as_i64().unwrap_or(0), effect["duration"].as_i64().unwrap_or(0) / 20));
            }
        } else if is_player_head(stack) {
            if let Some(name) = stack.get_component("minecraft:profile").and_then(|profile| profile["name"].as_str().or(profile.as_str())) {
                text.push_str(&format!("player:{name} "));
            }
        } else if is_leather_armor(material)
            && let Some(rgb) = stack.get_component("minecraft:dyed_color").and_then(|color| color["rgb"].as_i64().or(color.as_i64()))
        {
            text.push_str(&format!("color:{rgb} "));
        }
        text.trim().replace('\u{a7}', "&")
    }
}

fn serialize_lines(lines: &[String]) -> String {
    lines.iter().map(|line| unformat_string(line).replace(' ', "_").replace('|', "\\|")).collect::<Vec<_>>().join("|")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_real_file_loads() {
        let data = read_data(include_str!("../../resources/items.json"));
        assert!(data.item("stone").is_some());
        assert!(data.names.len() > 10_000);
        assert!(data.names.is_sorted_by(|left, right| left.0 < right.0));
        assert!(data.names.iter().all(|(_, item)| item.is_some()));
        assert_eq!(data.item("earth").map(|item| item.material.as_str()), Some("DIRT"));
    }
}
