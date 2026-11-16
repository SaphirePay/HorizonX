//! AdvancedCore-style rewards, as VotingPlugin's files write them: Commands (console or Console/Player), Messages
//! (Player/Broadcast), Money (an amount or `min-max`), Points, EXP/EXPLevels, Items, Title, ActionBar, Sound,
//! Chance, RandomCommand, AdvancedPriority (first that wins, else Fallback), Random and named reward files
//! (`Rewards: [name]`). Given on the server thread.

use crate::settings::{self, colour, fill};
use horizon_bukkit::{ItemStack, Material, Player};
use serde_yaml::Value as Yaml;

fn rand_unit() -> f64 {
    use std::cell::Cell;
    thread_local!(static SEED: Cell<u64> = Cell::new(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(9) | 1));
    SEED.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        (x >> 11) as f64 / (1u64 << 53) as f64
    })
}

fn strs(v: Option<&Yaml>) -> Vec<String> {
    match v {
        Some(Yaml::Sequence(s)) => s.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect(),
        Some(Yaml::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn num(v: Option<&Yaml>) -> Option<f64> {
    match v? {
        Yaml::Number(n) => n.as_f64(),
        Yaml::String(s) => match s.split_once('-') {
            Some((a, b)) if !a.is_empty() => {
                let (a, b): (f64, f64) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
                Some((a + rand_unit() * (b - a + 1.0)).floor().min(b))
            }
            _ => s.trim().parse().ok(),
        },
        _ => None,
    }
}

fn console(line: &str) {
    horizon_bukkit::fam_core::dispatch_command_as(&horizon_bukkit::CommandSender::Console, line.trim_start_matches('/'));
}

/// The reward `r` given to `player` (online), `vars` filling `%player%`, `%site%`, `%amount%`...
pub fn give(player: &Player, r: &Yaml, vars: &[(&str, &str)]) {
    let name = player.get_name().to_owned();
    let mut all: Vec<(&str, &str)> = vec![("player", &name), ("PlayerName", &name), ("username", &name)];
    all.extend_from_slice(vars);
    give_inner(player, r, &all, 0);
}

fn give_inner(player: &Player, r: &Yaml, vars: &[(&str, &str)], depth: usize) {
    if depth > 8 || r.is_null() {
        return;
    }
    // A list of reward names (or a single name) means reward files.
    if let Some(name) = r.as_str() {
        if let Some(doc) = settings::get().reward_files.get(&name.to_ascii_lowercase()).cloned() {
            give_inner(player, &doc, vars, depth + 1);
        }
        return;
    }
    if let Some(list) = r.as_sequence() {
        for item in list {
            give_inner(player, item, vars, depth + 1);
        }
        return;
    }
    if let Some(chance) = num(r.get("Chance"))
        && rand_unit() * 100.0 >= chance
    {
        if let Some(f) = r.get("ChanceFailedRewards") {
            give_inner(player, f, vars, depth + 1);
        }
        return;
    }
    if let Some(perm) = r.get("Permission").and_then(|p| p.as_str()).filter(|p| !p.is_empty())
        && !player.has_permission(perm)
    {
        return;
    }
    let f = |s: &str| fill(s, vars);
    // Commands
    match r.get("Commands") {
        Some(Yaml::Mapping(m)) => {
            for c in strs(m.get("Console")) {
                console(&f(&c));
            }
            for c in strs(m.get("Player")) {
                player.perform_command(f(&c).trim_start_matches('/'));
            }
        }
        other => {
            for c in strs(other) {
                console(&f(&c));
            }
        }
    }
    let random = strs(r.get("RandomCommand"));
    if !random.is_empty() {
        let pick = &random[(rand_unit() * random.len() as f64) as usize % random.len()];
        console(&f(pick));
    }
    // Messages
    match r.get("Messages") {
        Some(Yaml::Mapping(m)) => {
            for t in strs(m.get("Player")) {
                if !t.is_empty() {
                    player.send_message(&colour(&f(&t)));
                }
            }
            for t in strs(m.get("Broadcast")) {
                if !t.is_empty() {
                    horizon_bukkit::broadcast_message(&colour(&f(&t)));
                }
            }
        }
        Some(Yaml::String(t)) if !t.is_empty() => player.send_message(&colour(&f(t))),
        _ => {}
    }
    if let Some(t) = r.get("Message").and_then(|x| x.as_str()).filter(|t| !t.is_empty()) {
        player.send_message(&colour(&f(t)));
    }
    // Money, points, experience
    if let Some(m) = num(r.get("Money")).filter(|m| *m != 0.0) {
        if m > 0.0 {
            horizon_kit::hooks::economy::deposit(player.get_name(), m);
        } else {
            horizon_kit::hooks::economy::withdraw(player.get_name(), -m);
        }
    }
    if let Some(p) = num(r.get("Points")).filter(|p| *p != 0.0) {
        crate::users::add_points(&player.get_unique_id().to_ascii_lowercase(), p as i64);
    }
    if let Some(x) = num(r.get("EXP")).or_else(|| num(r.get("Exp"))).filter(|x| *x != 0.0) {
        player.give_exp(x as i32);
    }
    if let Some(x) = num(r.get("EXPLevels")).filter(|x| *x != 0.0) {
        player.give_exp_levels(x as i32);
    }
    // Items
    if let Some(Yaml::Mapping(items)) = r.get("Items") {
        for (_, it) in items {
            if let Some(chance) = num(it.get("Chance"))
                && rand_unit() * 100.0 >= chance
            {
                continue;
            }
            if let Some(stack) = item(it, vars) {
                if let Some(left) = player.get_inventory().add_item(&stack) {
                    let at = player.get_location();
                    at.get_world().drop_item_naturally(&at, &left);
                }
            }
        }
    }
    // Title, action bar, sound
    if let Some(t) = r.get("Title")
        && t.get("Enabled").and_then(|x| x.as_bool()).unwrap_or(true)
    {
        let title = colour(&f(t.get("Title").and_then(|x| x.as_str()).unwrap_or("")));
        let sub = colour(&f(t.get("SubTitle").and_then(|x| x.as_str()).unwrap_or("")));
        let gi = |k: &str, d: u32| t.get(k).and_then(|x| x.as_u64()).map(|x| x as u32).unwrap_or(d);
        player.send_title(&title, &sub, gi("FadeIn", 10), gi("ShowTime", 40), gi("FadeOut", 10));
    }
    if let Some(a) = r.get("ActionBar") {
        let text = a.get("Message").and_then(|x| x.as_str()).or_else(|| a.as_str()).unwrap_or("");
        if !text.is_empty() {
            player.send_action_bar(&horizon_bukkit::adventure::legacy(&colour(&f(text))));
        }
    }
    if let Some(snd) = r.get("Sound")
        && snd.get("Enabled").and_then(|x| x.as_bool()).unwrap_or(true)
        && let Some(id) = snd.get("Sound").and_then(|x| x.as_str())
    {
        let v = snd.get("Volume").and_then(|x| x.as_f64()).unwrap_or(1.0) as f32;
        let p = snd.get("Pitch").and_then(|x| x.as_f64()).unwrap_or(1.0) as f32;
        player.play_sound(&player.get_location(), id, v, p);
    }
    // AdvancedPriority: the first sub-reward whose chance hits, else Fallback.
    if let Some(Yaml::Mapping(m)) = r.get("AdvancedPriority") {
        let mut done = false;
        for (k, sub) in m {
            if k.as_str().is_some_and(|k| k.eq_ignore_ascii_case("Fallback")) {
                continue;
            }
            let chance = num(sub.get("Chance")).unwrap_or(100.0);
            if rand_unit() * 100.0 < chance {
                let mut without = sub.clone();
                if let Some(map) = without.as_mapping_mut() {
                    map.remove("Chance");
                }
                give_inner(player, &without, vars, depth + 1);
                done = true;
                break;
            }
        }
        if !done && let Some(fb) = m.iter().find(|(k, _)| k.as_str().is_some_and(|k| k.eq_ignore_ascii_case("Fallback"))).map(|(_, v)| v) {
            give_inner(player, fb, vars, depth + 1);
        }
    }
    // Random: { Chance, Rewards, FallBack }
    if let Some(rnd) = r.get("Random") {
        let chance = num(rnd.get("Chance")).unwrap_or(100.0);
        if rand_unit() * 100.0 < chance {
            if let Some(rw) = rnd.get("Rewards") {
                give_inner(player, rw, vars, depth + 1);
            }
        } else if let Some(fb) = rnd.get("FallBack") {
            give_inner(player, fb, vars, depth + 1);
        }
    }
    if let Some(named) = r.get("Rewards") {
        give_inner(player, named, vars, depth + 1);
    }
}

/// An item from an AdvancedCore item section (Material, Amount, Name, Lore, Enchants).
pub fn item(it: &Yaml, vars: &[(&str, &str)]) -> Option<ItemStack> {
    let mat = it.get("Material").and_then(|x| x.as_str())?;
    let amount = num(it.get("Amount")).unwrap_or(1.0).max(1.0) as i32;
    let mut st = ItemStack::new(Material::of(mat), amount);
    if let Some(n) = it.get("Name").and_then(|x| x.as_str()) {
        st.set_display_name(&colour(&fill(n, vars)));
    }
    let lore: Vec<String> = strs(it.get("Lore")).iter().map(|l| colour(&fill(l, vars))).collect();
    if !lore.is_empty() {
        st.set_lore(lore);
    }
    if let Some(Yaml::Mapping(e)) = it.get("Enchants") {
        for (k, v) in e {
            if let (Some(k), Some(lv)) = (k.as_str(), v.as_u64()) {
                st.add_enchant(&format!("minecraft:{}", k.to_ascii_lowercase()), lv as u32);
            }
        }
    }
    Some(st)
}
