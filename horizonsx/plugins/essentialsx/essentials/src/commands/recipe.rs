//! `Commandrecipe`.
use crate::prelude::*;
use crate::adventure::from_char_name;
use crate::recipes::{self, Recipe};
use horizon_bukkit::Inventory;
use horizon_bukkit::core2::get_recipes_for;
use std::collections::BTreeMap;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("recipe") };

fn material_name(sender: &CommandSource, stack: Option<&ItemStack>) -> String {
    stack.map_or_else(|| sender.tl("recipeNothing", &[]), |stack| stack.get_type().name().replace('_', " ").to_lowercase())
}

fn run(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    let first = args.first().ok_or_else(EssError::not_enough)?;
    let item = if first.eq_ignore_ascii_case("hand") {
        let player = sender.get_player().ok_or_else(|| EssError::tl("consoleCannotUseCommand", &[]))?;
        inventories::get_item_in_hand(player).unwrap_or_else(inventories::air)
    } else {
        item_db().get(first)?
    };
    let mut number = 0;
    if let Some(second) = args.get(1) {
        fail_if!(!is_int(second), EssError::tl("invalidNumber", &[]));
        number = second.parse::<i32>().unwrap_or(1) - 1;
    }
    let found = get_recipes_for(&item);
    fail_if!(found.is_empty(), EssError::tl("recipeNone", crate::args![material_name(sender, Some(&item))]));
    let mut list = Vec::new();
    for recipe in &found {
        let result = recipe.get_result().unwrap_or_else(|| item.clone());
        match recipes::read(recipe) {
            Recipe::Transmute { input, material } => {
                for from in &input {
                    for with in &material {
                        list.push((Recipe::Shapeless(vec![from.clone(), with.clone()]), result.clone()));
                    }
                }
            }
            other => list.push((other, result)),
        }
    }
    let (selected, result) = usize::try_from(number).ok().and_then(|index| list.get(index)).ok_or_else(|| EssError::tl("recipeBadIndex", &[]))?;
    let name = material_name(sender, Some(&item));
    sender.send_tl("recipe", crate::args![name.clone(), number + 1, list.len()]);
    match selected {
        Recipe::Furnace(input) => sender.send_tl("recipeFurnace", crate::args![material_name(sender, input.as_ref())]),
        Recipe::Shaped { shape, ingredients } => shaped_recipe(sender, shape, ingredients, result),
        Recipe::Shapeless(ingredients) => {
            if list.len() == 1 && item.get_type().name() == "FIREWORK_ROCKET" {
                let rocket = ["GUNPOWDER", "PAPER", "FIREWORK_STAR"].map(|id| ItemStack::new(horizon_bukkit::Material::of(id), 1));
                shapeless_recipe(sender, &rocket, result);
            } else {
                shapeless_recipe(sender, ingredients, result);
            }
        }
        Recipe::Transmute { .. } | Recipe::Other => {}
    }
    if list.len() > 1 && args.len() == 1 {
        sender.send_tl("recipeMore", crate::args![label, first, name]);
    }
    Ok(())
}

/// `openWorkbench` with the recipe laid out, then `setRecipeSee`. This workbench is the plugin's own and
/// not the game's. The game's gives its grid to the player as it closes, which would hand over what's
/// shown, and `InventoryCloseEvent` can't empty it in time: see "Engine requests" in GAPS.md. Slot 0
/// is the result, which the game's would have worked out.
fn show_workbench(sender: &CommandSource, result: &ItemStack, grid: impl IntoIterator<Item = (usize, ItemStack)>) {
    let (Some(user), Some(player), Some(mut workbench)) = (sender.get_user(), sender.get_player(), Inventory::create_of_type("WORKBENCH", "Crafting")) else {
        return;
    };
    workbench.set_item(0, Some(result.clone()));
    for (slot, item) in grid {
        workbench.set_item(slot, Some(item));
    }
    player.close_inventory();
    player.open_inventory(&workbench);
    user.set_recipe_see(true);
}

fn shaped_recipe(sender: &CommandSource, shape: &[String], ingredients: &BTreeMap<char, ItemStack>, result: &ItemStack) {
    if sender.is_player() {
        let cells = shape.iter().enumerate().flat_map(|(row, line)| line.chars().enumerate().map(move |(column, mark)| (row * 3 + column + 1, mark)));
        show_workbench(sender, result, cells.filter_map(|(slot, mark)| Some((slot, ingredients.get(&mark)?.clone()))));
        return;
    }
    let mut colors: Vec<(Option<String>, char)> = Vec::new();
    let mut next = 1u8;
    let mut number_of = |material: Option<String>| {
        if !colors.iter().any(|(known, _)| *known == material) {
            colors.push((material, char::from(b'0' + next)));
            next += 1;
        }
    };
    for mark in "abcdefghi".chars() {
        number_of(ingredients.get(&mark).map(|item| item.get_type().name()));
    }
    let mut grid: [[Option<String>; 3]; 3] = Default::default();
    for (row, line) in shape.iter().enumerate().take(3) {
        for (column, mark) in line.chars().enumerate().take(3) {
            grid[row][column] = ingredients.get(&mark).map(|item| item.get_type().name());
        }
    }
    let tag = |cell: &Option<String>| {
        let mark = colors.iter().find(|(known, _)| known == cell).map_or('0', |(_, mark)| *mark);
        parsed(format!("<{}>{mark}", from_char_name(mark).unwrap_or("white")))
    };
    for row in &grid {
        sender.send_tl("recipeGrid", &[tag(&row[0]), tag(&row[1]), tag(&row[2])]);
    }
    let mut where_list = String::new();
    for (material, mark) in &colors {
        let shown = material.as_ref().map_or_else(|| sender.tl("recipeNothing", &[]), |material| material.replace('_', " ").to_lowercase());
        where_list.push_str(&sender.tl("recipeGridItem", crate::args![mark.to_string(), shown]));
        where_list.push(' ');
    }
    sender.send_tl("recipeWhere", crate::args![parsed(where_list)]);
}

fn shapeless_recipe(sender: &CommandSource, ingredients: &[ItemStack], result: &ItemStack) {
    if sender.is_player() {
        show_workbench(sender, result, ingredients.iter().cloned().enumerate().map(|(index, item)| (index + 1, item)));
        return;
    }
    let names: Vec<String> = ingredients.iter().map(|item| material_name(sender, Some(item))).collect();
    sender.send_tl("recipeShapeless", crate::args![format!("{} ", names.join(", "))]);
}

fn tab(_sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 { get_items(args) } else { Vec::new() })
}
