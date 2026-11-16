//! Bounties: anyone can put money on another player's head (`/bounty set <player> <amount>`, taken from them at
//! once); whoever kills that player collects the whole pot. Bounties add up and stay until claimed.

use crate::{conf, data, teams, ui};
use horizon_bukkit::prelude::*;
use horizon_kit::store::File;
use horizon_kit::{app, gui, hooks::economy};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Duration;

const SECTION: &str = "Bounty";

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Bounty {
    pub name: String,
    pub amount: f64,
    /// Who put money on it, and how much.
    pub by: BTreeMap<String, f64>,
    pub since: u64,
}

fn file() -> &'static File<BTreeMap<String, Bounty>> {
    static FILE: OnceLock<File<BTreeMap<String, Bounty>>> = OnceLock::new();
    FILE.get_or_init(|| File::open(&app::folder().join("bounties.json"), Duration::from_secs(3)))
}

pub fn save() {
    file().save_now();
}

pub fn on(uuid: &str) -> f64 {
    file().read(|m| m.get(uuid).map_or(0.0, |b| b.amount))
}

fn set(p: &Player, target: &str, amount: &str) {
    if !crate::allowed(p, "horizonsmp.bounty.set") {
        return;
    }
    let s = conf::get();
    let Some(amount) = amount.replace([',', '$'], "").parse::<f64>().ok().filter(|a| a.is_finite() && *a > 0.0) else {
        return ui::say(p, SECTION, "<#f87171>The amount must be a number.");
    };
    let amount = amount.floor();
    if amount < s.bounty_min || amount > s.bounty_max {
        return ui::say(p, SECTION, &format!("<#f87171>A bounty is between {} and {}.", ui::money(s.bounty_min), ui::money(s.bounty_max)));
    }
    let Some((uuid, name)) = data::uuid_of(target) else {
        return ui::say(p, SECTION, &format!("<#f87171>No player named {} has played here.", ui::esc(target)));
    };
    if uuid == p.get_unique_id() {
        return ui::say(p, SECTION, "<#f87171>You can't put a bounty on yourself.");
    }
    if !economy::withdraw(p.get_name(), amount) {
        return ui::say(p, SECTION, &format!("<#f87171>You don't have {}.", ui::money(amount)));
    }
    let by = p.get_name().to_owned();
    let total = file().edit(|m| {
        let b = m.entry(uuid.clone()).or_insert_with(|| Bounty { name: name.clone(), amount: 0.0, by: BTreeMap::new(), since: horizon_kit::time::now_ms() });
        b.amount += amount;
        *b.by.entry(by.clone()).or_insert(0.0) += amount;
        b.amount
    });
    horizon_bukkit::server::send_rich_message(&format!(
        "<#f87171><bold>☠ BOUNTY</bold> <#3f3f46>» <white>{by}</white> <#a1a1aa>put <#facc15>{}</#facc15> on <white>{name}</white><#a1a1aa>'s head. Total: <#facc15><bold>{}</bold>",
        ui::money(amount),
        ui::money(total)
    ));
    for o in horizon_bukkit::get_online_players() {
        ui::sound(&o, "ENTITY_WITHER_SPAWN:0.25:1.6");
    }
    if let Some(t) = horizon_bukkit::fam_ess::get_player_by_id(&uuid) {
        ui::title(&t, "<#f87171><bold>☠ HUNTED</bold>", &format!("<#a1a1aa>there's <#facc15>{}</#facc15> on your head", ui::money(total)));
    }
}

/// A player killed another: the killer takes the bounty (not from a teammate, nor themselves).
pub fn on_kill(killer: &Player, victim: &Player) {
    if !conf::get().bounty {
        return;
    }
    let (ku, vu) = (killer.get_unique_id(), victim.get_unique_id());
    if ku == vu || teams::same_team(&ku, &vu) {
        return;
    }
    let Some(b) = file().edit(|m| m.remove(&vu)) else { return };
    economy::deposit(killer.get_name(), b.amount);
    horizon_bukkit::server::send_rich_message(&format!(
        "<#f87171><bold>☠ BOUNTY</bold> <#3f3f46>» <white>{}</white> <#a1a1aa>claimed the <#facc15><bold>{}</bold></#facc15> bounty on <white>{}</white>!",
        killer.get_name(),
        ui::money(b.amount),
        victim.get_name()
    ));
    ui::title(killer, "<#facc15><bold>BOUNTY CLAIMED</bold>", &format!("<#4ade80>+{}", ui::money(b.amount)));
    ui::fanfare(killer);
}

pub fn open(p: &Player) {
    if !crate::allowed(p, "horizonsmp.bounty") {
        return;
    }
    open_page(p, 0);
}

fn open_page(p: &Player, page: usize) {
    let mut list: Vec<(String, Bounty)> = file().read(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
    list.sort_by(|a, b| b.1.amount.total_cmp(&a.1.amount));
    let mut m = ui::frame(6, "☠ Bounties");
    m.item(4, ui::icon("WITHER_SKELETON_SKULL", "<#f87171><bold>Most wanted", &[
        String::new(),
        format!("<#a1a1aa>Open bounties  <white>{}", list.len()),
        format!("<#a1a1aa>Total reward    <#facc15>{}", ui::money(list.iter().map(|b| b.1.amount).sum())),
        String::new(),
        "<#71717a>Kill a wanted player to collect.".into(),
        "<#71717a>Place one: /bounty set <player> <amount>".into(),
    ]));
    if list.is_empty() {
        m.item(22, ui::icon("GRAY_DYE", "<#a1a1aa>No bounties", &ui::lines(&["<#71717a>Everyone is safe... for now."])));
    }
    let pages = gui::Pages::standard();
    pages.draw(
        &mut m,
        page,
        &list,
        |(uuid, b)| {
            let online = horizon_bukkit::fam_ess::get_player_by_id(uuid).is_some();
            let mut top: Vec<(&String, &f64)> = b.by.iter().collect();
            top.sort_by(|x, y| y.1.total_cmp(x.1));
            let mut lore = vec![String::new(), format!("<#a1a1aa>Reward   <#facc15><bold>{}", ui::money(b.amount)), format!("<#a1a1aa>Status    {}", if online { "<#4ade80>● online" } else { "<#71717a>● offline" }), String::new(), "<#71717a>Placed by:".into()];
            for (who, amt) in top.into_iter().take(4) {
                lore.push(format!("<#71717a> • <white>{who}</white> {}", ui::money(*amt)));
            }
            ui::head(&b.name, &format!("<#f87171><bold>{}", b.name), &lore)
        },
        |_c, _b| {},
        |pl, pg| open_page(pl, pg),
    );
    ui::back_button(&mut m, "the server menu", crate::menu::open);
    ui::close_button(&mut m);
    m.open(p);
}

pub fn bounty_cmd(sender: &CommandSender, args: &[&str]) -> bool {
    let Some(p) = sender.as_player() else { return true };
    let p = p.clone();
    if !conf::get().bounty {
        ui::say(&p, SECTION, "<#f87171>Bounties are turned off.");
        return true;
    }
    match args.first().map(|s| s.to_ascii_lowercase()).as_deref() {
        None | Some("list" | "menu" | "top") => open(&p),
        Some("set" | "place" | "add") => match (args.get(1), args.get(2)) {
            (Some(t), Some(a)) => set(&p, t, a),
            _ => ui::say(&p, SECTION, "Usage: <white>/bounty set <player> <amount>"),
        },
        Some("check" | "info") => {
            let name = args.get(1).copied().unwrap_or(p.get_name());
            match data::uuid_of(name) {
                Some((u, n)) => {
                    let a = on(&u);
                    if a > 0.0 {
                        ui::say(&p, SECTION, &format!("<white>{n}</white> has a <#facc15>{}</#facc15> bounty.", ui::money(a)));
                    } else {
                        ui::say(&p, SECTION, &format!("<white>{n}</white> has no bounty."));
                    }
                }
                None => ui::say(&p, SECTION, "<#f87171>No such player."),
            }
        }
        // `/bounty <player> <amount>` works too.
        Some(_) if args.len() >= 2 => set(&p, args[0], args[1]),
        _ => ui::say(&p, SECTION, "<white>/bounty</white> <#71717a>wanted list •</#71717a> <white>/bounty set <player> <amount></white> <#71717a>•</#71717a> <white>/bounty check <player>"),
    }
    true
}

pub fn complete(args: &[&str]) -> Vec<String> {
    match args {
        [one] => ["set", "check", "list"].iter().filter(|s| s.starts_with(&one.to_ascii_lowercase())).map(|s| (*s).to_owned()).collect(),
        [_, two] => horizon_bukkit::get_online_players().into_iter().map(|p| p.get_name().to_owned()).filter(|n| n.to_ascii_lowercase().starts_with(&two.to_ascii_lowercase())).collect(),
        _ => Vec::new(),
    }
}
