//! `/jobs browse` (the jobs to pick from: left click info, right click join, middle click leave) and
//! `/jobs shop` (points shop from shopItems.yml) as chest menus.

use crate::model;
use crate::settings::{self, Action};
use horizon_bukkit::fam_ev_player::InventoryClickEvent;
use horizon_bukkit::{Inventory, ItemStack, Material, Player};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

#[derive(Clone)]
enum Slot {
    Job(String),
    Shop(String),
}

struct Menu {
    inv: Inventory,
    slots: HashMap<usize, Slot>,
}

static MENUS: LazyLock<Mutex<HashMap<String, Menu>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn item(id: &str, name: &str, lore: Vec<String>) -> ItemStack {
    let id = settings::modern(id.split(';').next().unwrap_or("book"));
    let mut st = ItemStack::new(Material::of(&id), 1);
    st.set_display_name(&horizon_kit::text::colorize(name));
    st.set_lore(lore.iter().map(|l| horizon_kit::text::colorize(l)).collect());
    st
}

pub fn browse(player: &Player) {
    let s = settings::get();
    let uuid = player.get_unique_id().to_ascii_lowercase();
    let (joined, perms) = {
        let ps = model::players();
        match ps.get(&uuid) {
            Some(jp) => (jp.jobs.iter().map(|p| p.job.clone()).collect::<Vec<_>>(), jp.join_perm.clone()),
            None => return,
        }
    };
    let jobs: Vec<&settings::Job> = s.jobs.iter().filter(|j| !s.hide_without_perm || perms.contains(&j.name.to_ascii_lowercase())).collect();
    let rows = if s.gui_rows > 0 { s.gui_rows as usize } else { (jobs.len() / 7 + 3).clamp(3, 6) };
    let mut inv = Inventory::create(rows * 9, &s.msg("command.info.gui.pickjob", &[]));
    let mut slots = HashMap::new();
    let mut slot = 10;
    for j in jobs {
        if slot >= rows * 9 - 9 {
            break;
        }
        if slot % 9 == 8 {
            slot += 2;
        }
        let mut lore = vec![format!("&7{}", j.description)];
        lore.extend(j.full_description.iter().map(|l| format!("&7{l}")));
        if j.max_level > 0 {
            lore.push(format!("{}{}", s.msg("command.info.gui.max", &[]), j.max_level));
        }
        let workers = model::workers_of(&j.name);
        lore.push(format!("&7Workers: &e{workers}"));
        if joined.iter().any(|x| x.eq_ignore_ascii_case(&j.name)) {
            lore.push(s.msg("command.info.gui.working", &[]));
            lore.push(s.msg("command.info.gui.middleClick", &[]));
        } else {
            lore.push(s.msg("command.info.gui.rightClick", &[]));
        }
        lore.push(s.msg("command.info.gui.leftClick", &[]));
        let name = format!("{}{}", settings::chat_color(&j.color), j.display);
        inv.set_item(slot, Some(item(&j.gui_item, &name, lore)));
        slots.insert(slot, Slot::Job(j.name.clone()));
        slot += 1;
    }
    player.open_inventory(&inv);
    MENUS.lock().unwrap_or_else(|e| e.into_inner()).insert(uuid, Menu { inv, slots });
}

pub fn shop(player: &Player) {
    let s = settings::get();
    let uuid = player.get_unique_id().to_ascii_lowercase();
    let points = model::players().get(&uuid).map(|j| j.points).unwrap_or(0.0);
    let items: Vec<&settings::ShopItem> = s.shop.iter().filter(|i| !i.hide_without_perm || i.perms.iter().all(|p| player.has_permission(p))).collect();
    let rows = (items.len() / 9 + 1).clamp(1, 6);
    let mut inv = Inventory::create(rows * 9, &s.msg("command.shop.info.title", &[]));
    let mut slots = HashMap::new();
    for (i, it) in items.iter().enumerate().take(rows * 9) {
        let mut lore = it.lore.clone();
        lore.push(s.msg("command.shop.info.pointsPrice", &[("currentpoints", &s.points_fmt(points)), ("price", &s.points_fmt(it.points))]));
        inv.set_item(i, Some(item(&it.icon, &it.icon_name, lore)));
        slots.insert(i, Slot::Shop(it.key.clone()));
    }
    player.open_inventory(&inv);
    MENUS.lock().unwrap_or_else(|e| e.into_inner()).insert(uuid, Menu { inv, slots });
}

pub fn on_click(e: &mut InventoryClickEvent) {
    let Some(gui) = e.get_inventory().gui() else { return };
    let Some(player) = e.get_who_clicked() else { return };
    let uuid = player.get_unique_id().to_ascii_lowercase();
    let found = {
        let menus = MENUS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(menu) = menus.get(&uuid) else { return };
        if !menu.inv.same_as(gui) {
            return;
        }
        usize::try_from(e.get_raw_slot()).ok().and_then(|s| menu.slots.get(&s).cloned())
    };
    e.set_cancelled(true);
    let Some(slot) = found else { return };
    let click = e.get_click();
    crate::later(move || match slot {
        Slot::Job(name) => {
            player.close_inventory();
            match click {
                "RIGHT" | "SHIFT_RIGHT" => crate::commands::do_join(&player, &name),
                "MIDDLE" | "DROP" => crate::commands::do_leave(&player, &name),
                _ => crate::commands::show_info(&horizon_bukkit::CommandSender::Player(player.clone()), &name, None),
            }
        }
        Slot::Shop(key) => {
            player.close_inventory();
            buy(&player, &key);
        }
    });
}

pub fn closed(uuid: &str) {
    MENUS.lock().unwrap_or_else(|e| e.into_inner()).remove(uuid);
}

fn buy(player: &Player, key: &str) {
    let s = settings::get();
    let Some(it) = s.shop.iter().find(|i| i.key == key).cloned() else { return };
    let uuid = player.get_unique_id().to_ascii_lowercase();
    if !it.perms.iter().all(|p| player.has_permission(p)) {
        player.send_message(&s.msg("command.shop.info.NoPermForItem", &[]));
        return;
    }
    let ok = {
        let mut ps = model::players();
        let Some(jp) = ps.get_mut(&uuid) else { return };
        for (job, lvl) in &it.job_levels {
            if !jp.jobs.iter().any(|p| p.job.eq_ignore_ascii_case(job) && p.level >= *lvl) {
                drop(ps);
                player.send_message(&s.msg("command.shop.info.NoJobReqForitem", &[("jobname", job), ("joblevel", &lvl.to_string())]));
                return;
            }
        }
        if it.total_levels > 0 && jp.total_level() < it.total_levels {
            drop(ps);
            player.send_message(&s.msg("command.shop.info.NoTotalLevel", &[("totalLevel", &it.total_levels.to_string())]));
            return;
        }
        if jp.points < it.points {
            false
        } else {
            jp.points -= it.points;
            jp.dirty = true;
            true
        }
    };
    if !ok {
        player.send_message(&s.msg("command.shop.info.NoPoints", &[]));
        return;
    }
    for c in &it.commands {
        let line = c.replace("[player]", player.get_name()).replace("[playerName]", player.get_name());
        let line = if let Some(rest) = line.strip_prefix("msg ") { format!("tellraw {} {}", player.get_name(), serde_json::json!({"text": horizon_kit::text::colorize(rest)})) } else { line };
        model::run_command(&line, player);
    }
    for g in &it.give {
        let mut parts = g.split(';');
        let id = settings::modern(parts.next().unwrap_or("stone"));
        let amount = parts.next().and_then(|a| a.parse().ok()).unwrap_or(1);
        let mut st = ItemStack::new(Material::of(&id), amount);
        if let Some(name) = parts.next().filter(|n| !n.starts_with("n{")) {
            st.set_display_name(&horizon_kit::text::colorize(&name.replace('_', " ")));
        }
        if let Some(left) = player.get_inventory().add_item(&st) {
            let at = player.get_location();
            at.get_world().drop_item_naturally(&at, &left);
        }
    }
    player.send_message(&s.msg("command.shop.info.Paid", &[("amount", &s.points_fmt(it.points))]));
    let _ = Action::Break;
}
