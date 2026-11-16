//! The `Recipe` classes `/condense` and `/recipe` look at, read from the game's own recipe data.
use horizon_bukkit::core2::GameRecipe;
use horizon_bukkit::{ItemStack, Material};
use serde_json::Value;
use std::collections::BTreeMap;

pub enum Recipe {
    /// `FurnaceRecipe`: its input.
    Furnace(Option<ItemStack>),
    /// `ShapedRecipe`: `getShape` and `getIngredientMap`.
    Shaped { shape: Vec<String>, ingredients: BTreeMap<char, ItemStack> },
    /// `ShapelessRecipe`: `getIngredientList`.
    Shapeless(Vec<ItemStack>),
    /// `TransmuteRecipe`: every choice of its two inputs.
    Transmute { input: Vec<ItemStack>, material: Vec<ItemStack> },
    Other,
}

fn stack_of(id: &str) -> ItemStack {
    ItemStack::new(Material::of(id), 1)
}

/// The items one ingredient can be: an id, `#tag`, or a list of those.
fn choices(value: &Value) -> Vec<ItemStack> {
    match value {
        Value::String(id) => match id.strip_prefix('#') {
            Some(tag) => horizon_bukkit::registry::Tag::get("item", tag).get_values().iter().map(|id| stack_of(id)).collect(),
            None => vec![stack_of(id)],
        },
        Value::Array(list) => list.iter().flat_map(choices).collect(),
        Value::Object(object) => object.get("item").or(object.get("tag")).map_or_else(Vec::new, choices),
        _ => Vec::new(),
    }
}

/// `RecipeChoice.getItemStack`: the first of them.
fn first_choice(value: &Value) -> Option<ItemStack> {
    choices(value).into_iter().next()
}

/// The class a recipe of the game would have in Bukkit.
pub fn read(recipe: &GameRecipe) -> Recipe {
    let data = &recipe.data;
    match data["type"].as_str().unwrap_or_default().trim_start_matches("minecraft:") {
        "smelting" => Recipe::Furnace(first_choice(&data["ingredient"])),
        "crafting_shaped" => Recipe::Shaped {
            shape: data["pattern"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect(),
            ingredients: data["key"].as_object().into_iter().flatten().filter_map(|(key, value)| Some((key.chars().next()?, first_choice(value)?))).collect(),
        },
        "crafting_shapeless" => Recipe::Shapeless(data["ingredients"].as_array().into_iter().flatten().filter_map(first_choice).collect()),
        "crafting_transmute" => Recipe::Transmute { input: choices(&data["input"]), material: choices(&data["material"]) },
        _ => Recipe::Other,
    }
}
