//! Requirements (`RequirementList.evaluate` and each `Requirement.evaluate`).

use crate::Holder;
use crate::menu::{Check, Compare, ItemCheck, Requirement, RequirementList};
use horizon_bukkit::ItemStack;
use horizon_kit::hooks::economy;
use horizon_kit::text::colorize;

/// `RequirementList.evaluate`: each one's success/deny actions run as Java runs them; the list's own deny
/// actions are the caller's.
pub fn evaluate(list: &RequirementList, h: &Holder) -> bool {
    let mut ok = 0usize;
    for r in &list.list {
        if check(r, h) {
            ok += 1;
            crate::action::run(&r.success, h);
            if list.stop_at_success && ok >= list.minimum {
                break;
            }
        } else {
            crate::action::run(&r.deny, h);
            if !r.optional {
                return false;
            }
        }
    }
    ok >= list.minimum
}

fn check(r: &Requirement, h: &Holder) -> bool {
    let v = &h.viewer;
    let pass = match &r.check {
        Check::Permission(p) => v.has_permission(&h.fill(p)),
        Check::Permissions(perms, min) => {
            let count = perms.iter().filter(|p| v.has_permission(&h.fill(p))).count() as i64;
            let need = if *min < 1 { perms.len() as i64 } else { *min };
            if r.invert {
                return count + need <= perms.len() as i64;
            }
            count >= need
        }
        Check::Money { amount, placeholder } => {
            let amount = placeholder.as_ref().and_then(|p| h.fill(p).trim().parse::<f64>().ok()).unwrap_or(*amount);
            economy::has(v.get_name(), amount)
        }
        Check::Exp { amount, level } => {
            let Ok(need) = h.fill(amount).trim().parse::<i64>() else { return false };
            let has = if *level { i64::from(v.get_level()) } else { i64::from(v.get_total_experience()) };
            has >= need
        }
        Check::Item(c) => has_item(c, h),
        Check::Meta { key, kind, value } => {
            let key = h.fill(key);
            let Some(actual) = crate::meta::get(&v.get_unique_id(), &key, kind) else { return r.invert };
            let expected = h.fill(value);
            match kind.as_str() {
                "STRING" | "BOOLEAN" => actual.eq_ignore_ascii_case(&expected),
                "LONG" | "INTEGER" => match (actual.parse::<i64>(), expected.trim().parse::<i64>()) {
                    (Ok(a), Ok(e)) => a >= e,
                    _ => return r.invert,
                },
                "DOUBLE" => match (actual.parse::<f64>(), expected.trim().parse::<f64>()) {
                    (Ok(a), Ok(e)) => a >= e,
                    _ => return r.invert,
                },
                _ => return r.invert,
            }
        }
        Check::Near { location, distance } => match location {
            Some((world, x, y, z)) => {
                let at = v.get_location();
                v.get_world().get_name() == world && ((at.get_x() - x).powi(2) + (at.get_y() - y).powi(2) + (at.get_z() - z).powi(2)).sqrt() < *distance
            }
            None => false,
        },
        Check::Compare { kind, input, output } => {
            let a = h.fill(input);
            let b = h.fill(output);
            match kind {
                Compare::Contains => a.contains(&b),
                Compare::Equals => a == b,
                Compare::EqualsIgnoreCase => a.to_lowercase() == b.to_lowercase(),
                _ => {
                    let (Ok(x), Ok(y)) = (a.trim().parse::<f64>(), b.trim().parse::<f64>()) else { return false };
                    match kind {
                        Compare::Greater => x > y,
                        Compare::GreaterEq => x >= y,
                        Compare::Eq => x == y,
                        Compare::NotEq => x != y,
                        Compare::LessEq => x <= y,
                        _ => x < y,
                    }
                }
            }
        }
        Check::Regex { regex, input } => regex.is_match(&h.fill(&h.fill(input))),
        Check::Length { input, min, max } => {
            let n = h.fill(input).chars().count() as i64;
            n >= *min && max.is_none_or(|m| n <= m)
        }
        Check::IsObject { input, object } => {
            let t = h.fill(input);
            match object.as_str() {
                "int" => t.trim().parse::<i32>().is_ok(),
                "double" => t.trim().parse::<f64>().is_ok(),
                "player" => horizon_bukkit::get_online_players().iter().any(|p| p.get_name() == t || p.get_unique_id().eq_ignore_ascii_case(&t)),
                "uuid" => is_uuid(&t),
                _ => false,
            }
        }
        Check::Javascript(_) => false,
    };
    // Inverted kinds that are not compared above give the opposite.
    match &r.check {
        Check::Length { .. } | Check::IsObject { .. } | Check::Javascript(_) => pass,
        _ => pass != r.invert,
    }
}

fn is_uuid(t: &str) -> bool {
    let parts: Vec<&str> = t.split('-').collect();
    parts.len() == 5 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// `HasItemRequirement`: total of matching stacks in storage (+ armour, off hand) against the amount.
fn has_item(c: &ItemCheck, h: &Holder) -> bool {
    let material = h.fill(&c.material).to_ascii_uppercase();
    let inv = h.viewer.get_inventory();
    let contents = inv.get_contents();
    if material == "AIR" {
        let full = contents.iter().take(36).all(Option::is_some);
        return full;
    }
    let Some(mat) = horizon_bukkit::Material::get_material(&material) else { return false };
    let mut stacks: Vec<ItemStack> = contents.into_iter().take(36).flatten().collect();
    if c.armor {
        stacks.extend(inv.get_armor_contents().into_iter().flatten());
    }
    if c.offhand {
        stacks.extend(inv.get_item_in_off_hand());
    }
    let total: i64 = stacks.iter().filter(|s| s.get_type() == &mat && matches(c, s, h)).map(|s| i64::from(s.get_amount())).sum();
    total >= c.amount
}

fn text_match(have: &str, want: &str, contains: bool, ignore: bool) -> bool {
    match (contains, ignore) {
        (true, true) => have.to_lowercase().contains(&want.to_lowercase()),
        (true, false) => have.contains(want),
        (false, true) => have.eq_ignore_ascii_case(want),
        _ => have == want,
    }
}

fn matches(c: &ItemCheck, s: &ItemStack, h: &Holder) -> bool {
    if let Some(d) = c.data
        && i64::from(s.get_damage()) != d
    {
        return false;
    }
    if c.strict {
        return !s.has_custom_model_data() && !s.has_lore() && !s.has_display_name();
    }
    if c.model_data != 0 && s.get_custom_model_data().map(i64::from) != Some(c.model_data) {
        return false;
    }
    if let Some(name) = &c.name {
        let Some(have) = s.get_display_name() else { return false };
        if !text_match(&colorize(&h.fill(&have)), &colorize(&h.fill(name)), c.name_contains, c.name_ignorecase) {
            return false;
        }
    }
    if c.lore_list.is_some() || c.lore.is_some() {
        if !s.has_lore() {
            return false;
        }
        let have = s.get_lore().iter().map(|l| colorize(&h.fill(l))).collect::<Vec<_>>().join("&&");
        let want = match (&c.lore_list, &c.lore) {
            (Some(list), _) => list.iter().map(|l| colorize(&h.fill(l))).collect::<Vec<_>>().join("&&"),
            (None, Some(l)) => colorize(&h.fill(l)),
            _ => String::new(),
        };
        if !text_match(&have, &want, c.lore_contains, c.lore_ignorecase) {
            return false;
        }
    }
    true
}
