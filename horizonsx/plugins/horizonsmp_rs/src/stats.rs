//! `/stats [player]`: a player's profile; `/top`: leaderboards (playtime, kills, mobs, blocks mined, streaks).

use crate::{bounty, claims, data, teams, ui};
use horizon_bukkit::prelude::*;
use horizon_kit::hooks::economy;

fn kdr(k: u32, d: u32) -> String {
    if d == 0 { format!("{k}.00") } else { format!("{:.2}", f64::from(k) / f64::from(d)) }
}

pub fn open(p: &Player, target: Option<(String, String)>) {
    if !crate::allowed(p, "horizonsmp.stats") {
        return;
    }
    let (uuid, name) = target.unwrap_or_else(|| (p.get_unique_id(), p.get_name().to_owned()));
    let online = horizon_bukkit::fam_ess::get_player_by_id(&uuid).is_some();
    let d = if online { data::get(&uuid) } else { data::players().peek(&uuid, |d| d.clone()).unwrap_or_else(|| {
        let s = data::summaries().read(|m| m.get(&uuid).cloned()).unwrap_or_default();
        data::Profile { name: s.name, playtime: s.playtime, kills: s.kills, deaths: s.deaths, blocks_mined: s.blocks_mined, mob_kills: s.mob_kills, daily_streak: s.daily_streak, ..Default::default() }
    }) };
    let mut m = ui::frame(5, &format!("☺ {name}"));
    let team = teams::team_name(&uuid).unwrap_or_else(|| "none".into());
    let mut head = ui::head(&name, &ui::grad(&format!("<bold>{name}</bold>")), &[
        String::new(),
        format!("<#a1a1aa>Status   {}", if online { "<#4ade80>● online" } else { "<#71717a>● offline" }),
        format!("<#a1a1aa>Team      <white>{team}"),
        format!("<#a1a1aa>Balance  <#4ade80>{}", ui::money(economy::balance(&name))),
        if d.first_join > 0 { format!("<#a1a1aa>Joined    <white>{} ago", ui::duration(horizon_kit::time::now_ms().saturating_sub(d.first_join) / 1000)) } else { String::new() },
    ]);
    ui::glint(&mut head);
    m.item(4, head);
    let cells: [(usize, &str, String, Vec<String>); 7] = [
        (19, "CLOCK", "<#22d3ee><bold>Playtime".into(), vec![format!("<white>{}", ui::duration(d.playtime))]),
        (20, "DIAMOND_SWORD", "<#f87171><bold>PvP".into(), vec![format!("<#a1a1aa>Kills   <white>{}", d.kills), format!("<#a1a1aa>Deaths <white>{}", d.deaths), format!("<#a1a1aa>K/D     <white>{}", kdr(d.kills, d.deaths))]),
        (21, "ZOMBIE_HEAD", "<#4ade80><bold>Mobs killed".into(), vec![format!("<white>{}", ui::num(i64::from(d.mob_kills)))]),
        (22, "IRON_PICKAXE", "<#a1a1aa><bold>Blocks mined".into(), vec![format!("<white>{}", ui::num(d.blocks_mined as i64))]),
        (23, "GOLDEN_SHOVEL", "<#facc15><bold>Land".into(), vec![format!("<#a1a1aa>Claims  <white>{}", claims::claims_of(&uuid).len()), format!("<#a1a1aa>Blocks  <white>{} / {}", ui::num(claims::used_blocks(&uuid)), ui::num(claims::total_blocks(&uuid)))]),
        (24, "SUNFLOWER", "<#4ade80><bold>Daily streak".into(), vec![format!("<white>{} day(s)", d.daily_streak)]),
        (25, "WITHER_SKELETON_SKULL", "<#f87171><bold>Bounty".into(), vec![{ let b = bounty::on(&uuid); if b > 0.0 { format!("<#facc15>{}", ui::money(b)) } else { "<#71717a>none".into() } }]),
    ];
    for (slot, mat, title, mut lore) in cells {
        lore.insert(0, String::new());
        m.item(slot, ui::icon(mat, &title, &lore));
    }
    m.button(31, ui::icon("GOLD_BLOCK", "<#facc15><bold>Leaderboards", &ui::lines(&["", "<#22d3ee>▸ Click to see the top players"])), |c| {
        ui::click(&c.player);
        open_top(&c.player, 0);
    });
    ui::back_button(&mut m, "the server menu", crate::menu::open);
    ui::close_button(&mut m);
    m.open(p);
}

const BOARDS: [(&str, &str, &str); 5] = [("CLOCK", "Playtime", "playtime"), ("DIAMOND_SWORD", "Player kills", "kills"), ("ZOMBIE_HEAD", "Mob kills", "mobs"), ("IRON_PICKAXE", "Blocks mined", "mined"), ("SUNFLOWER", "Daily streak", "streak")];

fn value(s: &data::Summary, board: &str) -> i64 {
    match board {
        "playtime" => s.playtime as i64,
        "kills" => i64::from(s.kills),
        "mobs" => i64::from(s.mob_kills),
        "mined" => s.blocks_mined as i64,
        _ => i64::from(s.daily_streak),
    }
}

fn shown(v: i64, board: &str) -> String {
    match board {
        "playtime" => ui::duration(v.max(0) as u64),
        "streak" => format!("{v} day(s)"),
        _ => ui::num(v),
    }
}

pub fn open_top(p: &Player, which: usize) {
    if !crate::allowed(p, "horizonsmp.top") {
        return;
    }
    let which = which.min(BOARDS.len() - 1);
    let (_, title, board) = BOARDS[which];
    let mut rows: Vec<data::Summary> = data::summaries().read(|m| m.values().cloned().collect());
    rows.sort_by(|a, b| value(b, board).cmp(&value(a, board)));
    let mut m = ui::frame(6, &format!("★ Top • {title}"));
    for (i, (mat, name, _)) in BOARDS.iter().enumerate() {
        let mut icon = ui::icon(mat, &format!("{}<bold>{name}", if i == which { "<#facc15>" } else { "<#a1a1aa>" }), &ui::lines(&["", "<#22d3ee>▸ Click to show"]));
        if i == which {
            ui::glint(&mut icon);
        }
        m.button(2 + i, icon, move |c| {
            ui::click(&c.player);
            open_top(&c.player, i);
        });
    }
    let slots: Vec<usize> = vec![13, 21, 23, 28, 29, 30, 31, 32, 33, 34, 37, 38, 39, 40, 41, 42, 43];
    for (rank, (slot, s)) in slots.iter().zip(rows.iter().filter(|s| value(s, board) > 0)).enumerate() {
        let colour = match rank {
            0 => "<#facc15>",
            1 => "<#e4e4e7>",
            2 => "<#fb923c>",
            _ => "<#a1a1aa>",
        };
        let mut item = ui::head(&s.name, &format!("{colour}<bold>#{} {}", rank + 1, s.name), &[String::new(), format!("<#a1a1aa>{title}  <white>{}", shown(value(s, board), board))]);
        item.set_amount((rank + 1) as i32);
        m.item(*slot, item);
    }
    ui::back_button(&mut m, "the server menu", crate::menu::open);
    ui::close_button(&mut m);
    m.open(p);
}

pub fn stats_cmd(sender: &CommandSender, args: &[&str]) -> bool {
    let Some(p) = sender.as_player() else { return true };
    let p = p.clone();
    match args.first() {
        Some(_) if !crate::allowed(&p, "horizonsmp.stats.others") => {}
        Some(n) => match data::uuid_of(n) {
            Some(t) => open(&p, Some(t)),
            None => ui::say(&p, "Stats", "<#f87171>No player by that name has played here."),
        },
        None => open(&p, None),
    }
    true
}

pub fn top_cmd(sender: &CommandSender, args: &[&str]) -> bool {
    let Some(p) = sender.as_player() else { return true };
    let which = args.first().and_then(|a| BOARDS.iter().position(|b| b.2.starts_with(&a.to_ascii_lowercase()) || b.1.to_ascii_lowercase().starts_with(&a.to_ascii_lowercase()))).unwrap_or(0);
    open_top(&p.clone(), which);
    true
}

pub fn on_break(e: &mut BlockBreakEvent) {
    data::edit(&e.get_player().get_unique_id(), |d| d.blocks_mined += 1);
}
