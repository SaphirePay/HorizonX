//! A player's inventory as plugins use it: count what matches, take some, see how many more fit, give with the
//! rest dropped at their feet. Works on the 36 storage slots (hotbar + main), never armour or the off hand.
//!
//! ```ignore
//! let coal = |s: &ItemStack| s.get_type().name() == "COAL";
//! if inv::count(&p, coal) >= 8 { inv::take(&p, coal, 8); }
//! inv::give(&p, &ItemStack::new(Material::of("DIAMOND"), 3));
//! let fits = inv::room_for(&p, "OAK_LOG");
//! ```
//!
//! Each call reads the inventory once (one query) and writes only the slots it changes. Server thread only.

use horizon_bukkit::{ItemStack, Material, Player};

/// How many items in the storage slots `pick` accepts.
pub fn count(p: &Player, pick: impl Fn(&ItemStack) -> bool) -> i64 {
    p.get_inventory().get_storage_contents().iter().flatten().filter(|s| pick(s)).map(|s| i64::from(s.get_amount())).sum()
}

/// Takes up to `n` items that `pick` accepts (the held slot first); returns how many went and the stacks they
/// came from (each with the amount taken), e.g. to pay for them.
pub fn take(p: &Player, pick: impl Fn(&ItemStack) -> bool, n: i64) -> (i64, Vec<ItemStack>) {
    let inv = p.get_inventory();
    let contents = inv.get_storage_contents();
    let held = inv.get_held_item_slot().max(0) as usize;
    let mut order: Vec<usize> = vec![held];
    order.extend((0..contents.len()).filter(|s| *s != held));
    let (mut taken, mut out) = (0i64, Vec::new());
    for slot in order {
        if taken >= n {
            break;
        }
        let Some(Some(s)) = contents.get(slot) else { continue };
        if !pick(s) {
            continue;
        }
        let k = i64::from(s.get_amount()).min(n - taken);
        taken += k;
        let mut part = s.clone();
        part.set_amount(k as i32);
        out.push(part);
        if k >= i64::from(s.get_amount()) {
            inv.clear_slot(slot as i32);
        } else {
            let mut rest = s.clone();
            rest.set_amount(s.get_amount() - k as i32);
            inv.set_item(slot as i32, &rest);
        }
    }
    (taken, out)
}

/// Every item `pick` accepts, taken: (count, the stacks).
pub fn take_all(p: &Player, pick: impl Fn(&ItemStack) -> bool) -> (i64, Vec<ItemStack>) {
    take(p, pick, i64::MAX)
}

/// How many more of a plain `material` fit (partial stacks of it and empty slots).
pub fn room_for(p: &Player, material: &str) -> i64 {
    let probe = ItemStack::new(Material::of(material), 1);
    let max = i64::from(probe.get_max_stack_size().max(1));
    p.get_inventory()
        .get_storage_contents()
        .iter()
        .map(|s| match s {
            None => max,
            Some(s) if s.is_similar(&probe) => (max - i64::from(s.get_amount())).max(0),
            Some(_) => 0,
        })
        .sum()
}

/// Gives a stack; what doesn't fit drops at the player's feet. True when it all fit.
pub fn give(p: &Player, stack: &ItemStack) -> bool {
    match p.get_inventory().add_item(stack) {
        Some(left) if left.get_amount() > 0 => {
            let l = p.get_location();
            l.get_world().drop_item_naturally(&l, &left);
            false
        }
        _ => true,
    }
}

/// Gives `n` of a material in full stacks (more than 64 is fine).
pub fn give_many(p: &Player, material: &str, n: i64) -> bool {
    let probe = ItemStack::new(Material::of(material), 1);
    let max = i64::from(probe.get_max_stack_size().max(1));
    let mut left = n;
    let mut fit = true;
    while left > 0 {
        let k = left.min(max);
        fit &= give(p, &ItemStack::new(Material::of(material), k as i32));
        left -= k;
    }
    fit
}

/// A plain item of `material`: no custom name, lore or enchantments (what shops buy back).
pub fn plain(s: &ItemStack, material: &str) -> bool {
    s.get_type().name().eq_ignore_ascii_case(material) && !s.has_display_name() && !s.has_lore() && !s.has_enchants()
}
