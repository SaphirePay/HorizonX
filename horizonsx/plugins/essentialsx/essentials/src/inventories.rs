//! `craftbukkit/Inventories`: slots by Bukkit's numbers, 0-35 storage, 36-39 boots to helmet, 40 off hand.
use crate::utils::material::{is_boots, is_chestplate, is_helmet, is_leggings};
use horizon_bukkit::{ItemStack, Player};
use std::collections::BTreeMap;

const HELM_SLOT: i32 = 39;
const CHEST_SLOT: i32 = 38;
const LEG_SLOT: i32 = 37;
const BOOT_SLOT: i32 = 36;
const OFF_HAND_SLOT: i32 = 40;
const BODY_SLOT: i32 = 41;
const SADDLE_SLOT: i32 = 42;
const INVENTORY_SIZE: i32 = 41;

/// What `addItem` couldn't place, by the argument's index.
pub type Leftover = BTreeMap<usize, ItemStack>;

fn is_empty(stack: Option<&ItemStack>) -> bool {
    stack.is_none_or(|stack| stack.get_type().is_air())
}

/// `getItem`, for every slot `getItem(int)` has in Bukkit.
fn get_slot(player: &Player, slot: i32) -> Option<ItemStack> {
    let inventory = player.get_inventory();
    match slot {
        HELM_SLOT => inventory.get_helmet(),
        CHEST_SLOT => inventory.get_chestplate(),
        LEG_SLOT => inventory.get_leggings(),
        BOOT_SLOT => inventory.get_boots(),
        OFF_HAND_SLOT => inventory.get_item_in_off_hand(),
        _ => inventory.get_item(slot),
    }
}

/// `setItem`, for every slot `setItem(int)` has in Bukkit.
pub fn set_slot(player: &Player, slot: i32, stack: Option<&ItemStack>) {
    let inventory = player.get_inventory();
    let air = ItemStack::new(horizon_bukkit::Material::of("AIR"), 1);
    let held = stack.filter(|stack| stack.get_amount() > 0).unwrap_or(&air);
    match slot {
        HELM_SLOT => inventory.set_helmet(stack),
        CHEST_SLOT => inventory.set_chestplate(stack),
        LEG_SLOT => inventory.set_leggings(stack),
        BOOT_SLOT => inventory.set_boots(stack),
        OFF_HAND_SLOT => inventory.set_item_in_off_hand(held),
        _ => inventory.set_item(slot, held),
    }
}

fn contents(player: &Player) -> Vec<Option<ItemStack>> {
    (0..INVENTORY_SIZE).map(|slot| get_slot(player, slot)).collect()
}

/// `getItemInHand`: the main hand, or the off hand if that's empty.
pub fn get_item_in_hand(player: &Player) -> Option<ItemStack> {
    let inventory = player.get_inventory();
    let main = inventory.get_item_in_main_hand();
    if is_empty(main.as_ref()) { inventory.get_item_in_off_hand() } else { main }
}

/// Writes back what `getItemInHand` gave: Java changes that stack in place.
pub fn set_item_in_hand(player: &Player, stack: &ItemStack) {
    let inventory = player.get_inventory();
    if is_empty(inventory.get_item_in_main_hand().as_ref()) {
        inventory.set_item_in_off_hand(stack);
    } else {
        inventory.set_item_in_main_hand(stack);
    }
}

/// `new ItemStack(Material.AIR)`, what clears a slot.
pub fn air() -> ItemStack {
    ItemStack::new(horizon_bukkit::Material::of("AIR"), 1)
}

/// `getItem(slot)`, for every slot Bukkit has.
pub fn get_item(player: &Player, slot: i32) -> Option<ItemStack> {
    get_slot(player, slot)
}

/// `getItemInMainHand`.
pub fn get_item_in_main_hand(player: &Player) -> Option<ItemStack> {
    player.get_inventory().get_item_in_main_hand()
}

/// `setItemInMainHand`.
pub fn set_item_in_main_hand(player: &Player, stack: &ItemStack) {
    player.get_inventory().set_item_in_main_hand(stack);
}

/// `containsAtLeast`.
pub fn contains_at_least(player: &Player, item: &ItemStack, amount: i32) -> bool {
    let mut left = amount;
    for held in contents(player).into_iter().flatten() {
        if !is_empty(Some(&held)) && held.is_similar(item) {
            left -= held.get_amount();
            if left <= 0 {
                return true;
            }
        }
    }
    false
}

/// `isContortedSlot`.
pub fn is_contorted_slot(slot: i32) -> bool {
    slot == BODY_SLOT || slot == SADDLE_SLOT
}

fn is_armor_slot(slot: i32) -> bool {
    (BOOT_SLOT..=HELM_SLOT).contains(&slot)
}

/// `isBottomInventorySlot`.
pub fn is_bottom_inventory_slot(slot: i32) -> bool {
    slot > 35
}

/// Runs `open` when the player's turn is over. A command or a sign's click is that turn, and a station
/// or a live view opened during it is lost when the turn puts the player's state back, leaving a
/// window on the client with nothing behind it: see "Engine requests" in GAPS.md.
pub fn after_turn(open: impl FnOnce() + Send + 'static) {
    crate::ess::ess().schedule_sync_delayed_task(open);
}

/// `InventoryData`: free slots, and for each new item the slots that can take more of it.
struct InventoryData {
    empty: Vec<i32>,
    partial: Vec<Vec<i32>>,
}

fn normalize(items: &[ItemStack]) -> Vec<ItemStack> {
    if items.len() <= 1 {
        return items.iter().filter(|item| !is_empty(Some(item))).cloned().collect();
    }
    let mut normalized: Vec<ItemStack> = Vec::new();
    for item in items {
        if is_empty(Some(item)) {
            continue;
        }
        match normalized.iter_mut().find(|known| item.is_similar(known)) {
            Some(known) => {
                let total = known.get_amount() + item.get_amount();
                known.set_amount(total);
            }
            None => normalized.push(item.clone()),
        }
    }
    normalized
}

fn parse_inventory_data(player: &Player, items: &[ItemStack], max_stack: i32, include_armor: bool) -> InventoryData {
    let mut data = InventoryData { empty: Vec::new(), partial: vec![Vec::new(); items.len()] };
    for (slot, held) in (0..).zip(contents(player)) {
        if is_contorted_slot(slot) || (!include_armor && is_armor_slot(slot)) {
            continue;
        }
        match held.filter(|held| !held.get_type().is_air()) {
            None => data.empty.push(slot),
            Some(held) => {
                for (index, item) in items.iter().enumerate() {
                    if held.get_amount() < max_stack.max(held.get_max_stack_size()) && held.is_similar(item) {
                        data.partial[index].push(slot);
                    }
                }
            }
        }
    }
    if include_armor {
        let mut seen = [false; 4];
        for (index, item) in items.iter().enumerate() {
            let material = item.get_type();
            let which = if is_helmet(material) {
                0
            } else if is_chestplate(material) {
                1
            } else if is_leggings(material) {
                2
            } else if is_boots(material) {
                3
            } else {
                continue;
            };
            if !seen[which] {
                seen[which] = true;
                let slot = [HELM_SLOT, CHEST_SLOT, LEG_SLOT, BOOT_SLOT][which];
                if data.empty.contains(&slot) {
                    data.partial[index].push(slot);
                }
            }
        }
        data.empty.retain(|slot| !is_armor_slot(*slot));
    }
    data
}

/// `hasSpace`.
pub fn has_space(player: &Player, max_stack: i32, include_armor: bool, items: &[ItemStack]) -> bool {
    let mut items = normalize(items);
    let mut data = parse_inventory_data(player, &items, max_stack, include_armor);
    for (index, item) in items.iter_mut().enumerate() {
        let item_max = max_stack.max(item.get_max_stack_size());
        loop {
            if data.partial[index].is_empty() {
                if data.empty.is_empty() {
                    return false;
                }
                data.empty.remove(0);
                if item.get_amount() > item_max {
                    let left = item.get_amount() - item_max;
                    item.set_amount(left);
                } else {
                    break;
                }
            } else {
                let slot = data.partial[index].remove(0);
                let existing = get_slot(player, slot).filter(|held| !held.get_type().is_air()).map_or(0, |held| held.get_amount());
                if item.get_amount() + existing <= item_max {
                    break;
                }
                let left = item.get_amount() + existing - item_max;
                item.set_amount(left);
            }
        }
    }
    true
}

/// `addItem(player, maxStack, allowArmor, items...)`.
pub fn add_item(player: &Player, max_stack: i32, allow_armor: bool, items: &[ItemStack]) -> Leftover {
    let mut items = normalize(items);
    let mut leftover = Leftover::new();
    let mut data = parse_inventory_data(player, &items, max_stack, allow_armor);
    // What this call placed: slots are written through commands, so they don't read back at once.
    let mut placed: BTreeMap<i32, ItemStack> = BTreeMap::new();
    for (index, item) in items.iter_mut().enumerate() {
        let item_max = max_stack.max(item.get_max_stack_size());
        loop {
            if data.partial[index].is_empty() {
                if data.empty.is_empty() {
                    leftover.insert(index, item.clone());
                    break;
                }
                let slot = data.empty.remove(0);
                if item.get_amount() > item_max {
                    let split = item.as_quantity(item_max);
                    set_slot(player, slot, Some(&split));
                    placed.insert(slot, split);
                    let left = item.get_amount() - item_max;
                    item.set_amount(left);
                } else {
                    set_slot(player, slot, Some(item));
                    placed.insert(slot, item.clone());
                    break;
                }
            } else {
                let slot = data.partial[index].remove(0);
                let mut existing = placed.get(&slot).cloned().or_else(|| get_slot(player, slot)).filter(|held| !held.get_type().is_air()).unwrap_or_else(|| item.as_quantity(0));
                let (amount, held) = (item.get_amount(), existing.get_amount());
                if amount + held <= item_max {
                    existing.set_amount(amount + held);
                    set_slot(player, slot, Some(&existing));
                    placed.insert(slot, existing);
                    break;
                }
                existing.set_amount(item_max);
                set_slot(player, slot, Some(&existing));
                placed.insert(slot, existing);
                item.set_amount(amount + held - item_max);
            }
        }
    }
    leftover
}

/// `addItem(player, maxStack, item, slot)`: what wouldn't fit in that slot.
pub fn add_item_at(player: &Player, max_stack: i32, item: ItemStack, slot: i32) -> Option<ItemStack> {
    let item_max = max_stack.max(item.get_max_stack_size());
    let held = get_slot(player, slot).filter(|held| !held.get_type().is_air());
    let existing_amount = held.as_ref().map_or(0, ItemStack::get_amount);
    let mut existing = held.unwrap_or_else(|| item.clone());
    if !item.is_similar(&existing) {
        return Some(item);
    }
    let amount = item.get_amount();
    if amount + existing_amount <= item_max {
        existing.set_amount(amount + existing_amount);
        set_slot(player, slot, Some(&existing));
        return None;
    }
    existing.set_amount(item_max);
    set_slot(player, slot, Some(&existing));
    let mut left = item;
    left.set_amount(amount + existing_amount - item_max);
    Some(left)
}

/// `getInventory`.
pub fn get_inventory(player: &Player, include_armor: bool) -> Vec<Option<ItemStack>> {
    (0..INVENTORY_SIZE).map(|slot| if !include_armor && is_armor_slot(slot) { None } else { get_slot(player, slot) }).collect()
}

/// `removeItems`.
pub fn remove_items(player: &Player, mut remove: impl FnMut(&ItemStack) -> bool, include_armor: bool) -> i32 {
    let mut removed = 0;
    for (slot, item) in (0..).zip(contents(player)) {
        if is_contorted_slot(slot) || (!include_armor && is_armor_slot(slot)) {
            continue;
        }
        if let Some(item) = item.filter(|item| !item.get_type().is_air())
            && remove(&item)
        {
            removed += item.get_amount();
            set_slot(player, slot, None);
        }
    }
    removed
}

/// `removeItemExact`.
pub fn remove_item_exact(player: &Player, target: &ItemStack, include_armor: bool) {
    remove_items(player, |item| item == target, include_armor);
}

/// `removeItemSimilar`.
pub fn remove_item_similar(player: &Player, target: &ItemStack, include_armor: bool) -> i32 {
    remove_items(player, |item| item.is_similar(target), include_armor)
}

/// `removeItemAmount`.
pub fn remove_item_amount(player: &Player, target: &ItemStack, amount: i32) -> bool {
    let mut left = amount;
    let mut clear = Vec::new();
    for (slot, item) in (0..).zip(contents(player)) {
        if is_contorted_slot(slot) {
            continue;
        }
        let Some(mut item) = item.filter(|item| !item.get_type().is_air()) else { continue };
        if item.is_similar(target) {
            if item.get_amount() >= left {
                item.set_amount(item.get_amount() - left);
                set_slot(player, slot, Some(&item));
                for slot in clear {
                    clear_slot(player, slot);
                }
                return true;
            }
            left -= item.get_amount();
            clear.push(slot);
            if left == 0 {
                for slot in clear {
                    clear_slot(player, slot);
                }
                return true;
            }
        }
    }
    false
}

/// `clearSlot`.
pub fn clear_slot(player: &Player, slot: i32) {
    set_slot(player, slot, None);
}
