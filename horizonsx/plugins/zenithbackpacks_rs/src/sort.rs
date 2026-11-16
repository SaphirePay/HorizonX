//! Tidy and search over every page of a vault chest (`ChestSortService`, `ChestSearchService`).

use horizon_bukkit::ItemStack;
use std::cmp::Ordering;

/// Coarse grouping from the material's name so like things end up together.
pub fn category_of(material: &str) -> &'static str {
    let n = material.to_ascii_uppercase();
    let ends = |s: &str| n.ends_with(s);
    if ["_PICKAXE", "_AXE", "_SHOVEL", "_HOE", "_SWORD"].iter().any(|s| ends(s)) || ["SHEARS", "FISHING_ROD", "FLINT_AND_STEEL"].contains(&n.as_str()) {
        return "TOOL";
    }
    if ["_HELMET", "_CHESTPLATE", "_LEGGINGS", "_BOOTS"].iter().any(|s| ends(s)) || ["SHIELD", "ELYTRA"].contains(&n.as_str()) {
        return "ARMOR";
    }
    if ["_APPLE", "_STEW", "_SOUP"].iter().any(|s| ends(s)) || ["BREAD", "CARROT", "POTATO", "MELON_SLICE"].contains(&n.as_str()) || n.starts_with("COOKED_") {
        return "FOOD";
    }
    if ["_INGOT", "_NUGGET", "_ORE"].iter().any(|s| ends(s)) || ["DIAMOND", "EMERALD", "COAL", "REDSTONE", "QUARTZ", "LAPIS_LAZULI"].contains(&n.as_str()) {
        return "ORE";
    }
    "BLOCK"
}

/// Category, then name, then bigger stacks first (`SortKey`).
pub fn order(a: (&str, i32), b: (&str, i32)) -> Ordering {
    category_of(a.0).cmp(category_of(b.0)).then_with(|| a.0.cmp(b.0)).then_with(|| b.1.cmp(&a.1))
}

fn name_of(s: &ItemStack) -> String {
    s.get_type().name()
}

/// Merge part stacks, order, refill from slot 0, over the first `capacity` slots.
pub fn sort(items: &mut [Option<ItemStack>], capacity: usize) {
    let cap = capacity.min(items.len());
    let gathered: Vec<ItemStack> = items[..cap].iter_mut().filter_map(Option::take).filter(|s| !s.is_empty()).collect();
    let mut merged: Vec<ItemStack> = Vec::new();
    for mut candidate in gathered {
        for target in &mut merged {
            let max = target.get_max_stack_size();
            if target.get_amount() < max && target.is_similar(&candidate) {
                let room = max - target.get_amount();
                let moved = room.min(candidate.get_amount());
                target.set_amount(target.get_amount() + moved);
                candidate.set_amount(candidate.get_amount() - moved);
                if candidate.get_amount() <= 0 {
                    break;
                }
            }
        }
        if candidate.get_amount() > 0 {
            merged.push(candidate);
        }
    }
    let mut keyed: Vec<(String, ItemStack)> = merged.into_iter().map(|s| (name_of(&s), s)).collect();
    keyed.sort_by(|a, b| order((&a.0, a.1.get_amount()), (&b.0, b.1.get_amount())));
    for (i, (_, s)) in keyed.into_iter().enumerate().take(cap) {
        items[i] = Some(s);
    }
}

/// "DIAMOND_SWORD" -> "diamond sword", so players type what they see.
pub fn normalise(material: &str) -> String {
    material.to_lowercase().replace('_', " ").trim().to_owned()
}

pub fn matches(haystack: &str, query: &str) -> bool {
    let needle = query.trim().to_lowercase();
    !needle.is_empty() && haystack.to_lowercase().contains(&needle)
}

/// Storage indexes of matching stacks, page order, at most one window's worth.
pub fn find(items: &[Option<ItemStack>], capacity: usize, query: &str, limit: usize) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, s) in items.iter().enumerate().take(capacity.min(items.len())) {
        if out.len() >= limit {
            break;
        }
        if let Some(s) = s.as_ref().filter(|s| !s.is_empty())
            && matches(&normalise(&name_of(s)), query)
        {
            out.push(i);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_and_order() {
        assert_eq!(category_of("DIAMOND_PICKAXE"), "TOOL");
        assert_eq!(category_of("IRON_BOOTS"), "ARMOR");
        assert_eq!(category_of("COOKED_BEEF"), "FOOD");
        assert_eq!(category_of("IRON_INGOT"), "ORE");
        assert_eq!(category_of("STONE"), "BLOCK");
        assert_eq!(order(("STONE", 5), ("IRON_INGOT", 1)), Ordering::Less);
        assert_eq!(order(("STONE", 5), ("STONE", 64)), Ordering::Greater);
    }

    #[test]
    fn search_words() {
        assert!(matches(&normalise("DIAMOND_SWORD"), "mond sw"));
        assert!(!matches("stone", "  "));
    }
}
