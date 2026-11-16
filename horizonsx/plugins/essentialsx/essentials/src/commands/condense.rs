//! `Commandcondense`.
use crate::prelude::*;
use crate::recipes::{self, Recipe};
use crate::trade::OverflowType;
use horizon_bukkit::core2::{get_recipes_for, recipe_iterator};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("condense") };

/// `SimpleRecipe`: what it makes, then what it takes.
type Simple = (ItemStack, ItemStack);

/// `condenseList`, by the stack as saved.
static CONDENSE_LIST: LazyLock<Mutex<HashMap<String, Option<Simple>>>> = LazyLock::new(Default::default);

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let Some(player) = user.player() else { return Ok(()) };
    let (stacks, validate_reverse) = if args.is_empty() {
        (inventories::get_inventory(&player, false).into_iter().flatten().filter(|stack| !stack.get_type().is_air()).collect(), true)
    } else {
        let words: Vec<&str> = args.iter().map(String::as_str).collect();
        (item_db().get_matching(user, &words)?, false)
    };
    let mut did_convert = false;
    for stack in &stacks {
        did_convert |= condense_stack(user, stack, validate_reverse)?;
    }
    if did_convert {
        user.send_tl("itemsConverted", &[]);
        Ok(())
    } else {
        user.send_tl("itemsNotConverted", &[]);
        Err(EssError::NoCharge)
    }
}

fn condense_stack(user: &User, stack: &ItemStack, validate_reverse: bool) -> EssResult<bool> {
    let Some((mut result, mut input)) = get_condense_type(stack) else { return Ok(false) };
    if validate_reverse && !get_recipes_for(&input).iter().any(|recipe| stack_on_recipe_match(&recipes::read(recipe), &result).is_some()) {
        return Ok(false);
    }
    let Some(player) = user.player() else { return Ok(false) };
    let mut amount: i32 = inventories::get_inventory(&player, false).into_iter().flatten().filter(|contents| contents.is_similar(stack)).map(|contents| contents.get_amount()).sum();
    let output = (amount / input.get_amount()) * result.get_amount();
    amount -= amount % input.get_amount();
    if amount <= 0 {
        return Ok(false);
    }
    input.set_amount(amount);
    result.set_amount(output);
    Trade::of_item(input).charge(user)?;
    Trade::of_item(result).pay_with(user, OverflowType::Drop)?;
    Ok(true)
}

fn get_condense_type(stack: &ItemStack) -> Option<Simple> {
    let key = stack.to_snbt();
    if let Some(known) = CONDENSE_LIST.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return known.clone();
    }
    let mut best: Vec<Simple> = Vec::new();
    for recipe in recipe_iterator() {
        let Some(result) = recipe.get_result() else { continue };
        if let Some(count) = stack_on_recipe_match(&recipes::read(&recipe), stack)
            && (count == 4 || count == 9)
            && count > result.get_amount()
        {
            let mut input = stack.clone();
            input.set_amount(count);
            best.push((result, input));
        }
    }
    best.sort_by_key(|(_, input)| std::cmp::Reverse(input.get_amount()));
    let chosen = best.into_iter().next();
    CONDENSE_LIST.lock().unwrap_or_else(|e| e.into_inner()).insert(key, chosen.clone());
    chosen
}

/// `getStackOnRecipeMatch`, as how many ingredients the recipe has.
fn stack_on_recipe_match(recipe: &Recipe, stack: &ItemStack) -> Option<i32> {
    let inputs: Vec<&ItemStack> = match recipe {
        Recipe::Shaped { shape, ingredients } => {
            if shape.len() != shape.first()?.chars().count() {
                return None;
            }
            shape.iter().flat_map(|row| row.chars()).filter_map(|mark| ingredients.get(&mark)).collect()
        }
        Recipe::Shapeless(ingredients) => ingredients.iter().collect(),
        _ => return None,
    };
    inputs.iter().all(|slot| slot.is_similar(stack)).then_some(inputs.len() as i32)
}

fn tab(_user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 { get_matching_items(args) } else { Vec::new() })
}
