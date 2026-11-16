//! Rewards for playing: a daily reward with a 7-day streak (`/daily`), one-off rewards at playtime milestones
//! (`/rewards`), and the first-join welcome (kit, money, a key, a broadcast). Playtime is counted here too.

use crate::{conf, data, ui};
use horizon_bukkit::prelude::*;
use std::collections::HashMap;
use std::sync::Mutex;

const SECTION: &str = "Rewards";
#[cfg(test)]
const DAY_MS: u64 = 86_400_000;
/// A daily reward is ready 20 hours after the last one; the streak survives up to 48 hours.
const READY_MS: u64 = 20 * 3_600_000;
const KEEP_MS: u64 = 48 * 3_600_000;

fn now() -> u64 {
    horizon_kit::time::now_ms()
}

// ---- Playtime ----

static SINCE: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);

fn since<R>(f: impl FnOnce(&mut HashMap<String, u64>) -> R) -> R {
    let mut held = SINCE.lock().unwrap_or_else(|e| e.into_inner());
    f(held.get_or_insert_with(HashMap::new))
}

/// Adds the time since the last count to the player's playtime.
fn count(uuid: &str) {
    let t = now();
    let last = since(|m| m.insert(uuid.to_owned(), t));
    if let Some(last) = last {
        let secs = t.saturating_sub(last) / 1000;
        if secs > 0 {
            // The few ms under a second are kept for the next count.
            since(|m| m.insert(uuid.to_owned(), last + secs * 1000));
            data::edit(uuid, |d| {
                d.playtime += secs;
                d.last_seen = t;
            });
        }
    }
}

/// Every 15 seconds.
pub fn tick() {
    for p in horizon_bukkit::get_online_players() {
        count(&p.get_unique_id());
    }
}

pub fn quit(uuid: &str) {
    count(uuid);
    since(|m| m.remove(uuid));
}

/// After a hot reload: count from now, no welcome.
pub fn join_quietly(p: &Player) {
    since(|m| m.insert(p.get_unique_id(), now()));
}

pub fn join(p: &Player) {
    let uuid = p.get_unique_id();
    since(|m| m.insert(uuid.clone(), now()));
    let first = data::edit(&uuid, |d| {
        d.name = p.get_name().to_owned();
        d.last_seen = now();
        if d.first_join == 0 {
            d.first_join = now();
            true
        } else {
            false
        }
    });
    let pl = p.clone();
    if first {
        horizon_kit::app::later(40, move || welcome(&pl));
    } else {
        horizon_kit::app::later(80, move || remind(&pl));
    }
}

fn welcome(p: &Player) {
    let s = conf::get();
    if !p.is_online() || !s.first_join {
        return;
    }
    for line in &s.first_join_kit {
        let mut w = line.split_whitespace();
        let Some(mat) = w.next().and_then(|m| Material::get_material(&m.to_ascii_uppercase())) else { continue };
        let n = w.next().and_then(|n| n.parse::<i32>().ok()).unwrap_or(1).max(1);
        let name: Vec<&str> = w.collect();
        let mut stack = ItemStack::new(mat, n);
        if !name.is_empty() {
            horizon_kit::item::name(&mut stack, &name.join(" "));
        }
        crate::give_item(p, &stack);
    }
    conf::give(p, &s.first_join_reward);
    let n = data::summaries().read(|m| m.len()) + 1;
    ui::title(p, &ui::grad(&format!("<bold>WELCOME TO {}</bold>", s.server.to_ascii_uppercase())), "<#a1a1aa>type <white>/menu</white> to get started");
    ui::fanfare(p);
    ui::raw(p, &[
        String::new(),
        format!("  {}", ui::grad(&format!("<bold>Welcome to {}, {}!</bold>", s.server, p.get_name()))),
        String::new(),
        "  <#a1a1aa>You got a <white>starter kit</white>, some money and a <white>Common Key</white>.".to_owned(),
        "  <#a1a1aa>New players are <#4ade80>safe from PvP</#4ade80> for a while: build a base!".to_owned(),
        String::new(),
        "  <#a855f7>▸</#a855f7> <click:run_command:/menu><hover:show_text:'Everything in one place'><#22d3ee><u>/menu</u></#22d3ee></hover></click> <#71717a>shop, crates, teams, rewards".to_owned(),
        "  <#a855f7>▸</#a855f7> <click:run_command:/claim><hover:show_text:'Protect your builds'><#22d3ee><u>/claim</u></#22d3ee></hover></click> <#71717a>protect your land (golden shovel)".to_owned(),
        "  <#a855f7>▸</#a855f7> <click:run_command:/daily><#22d3ee><u>/daily</u></#22d3ee></click> <#71717a>a reward every day".to_owned(),
        "  <#a855f7>▸</#a855f7> <click:run_command:/rules><#22d3ee><u>/rules</u></#22d3ee></click> <#71717a>please read them".to_owned(),
        String::new(),
    ].join("\n"));
    if s.first_join_broadcast {
        horizon_bukkit::server::send_rich_message(&format!("<gradient:{}:{}><bold>✦</bold></gradient> <#a1a1aa>Welcome <white>{}</white> to {}! <#71717a>(player #{})", ui::ACCENT, ui::ACCENT2, p.get_name(), s.server, ui::num(n as i64)));
    }
}

fn remind(p: &Player) {
    if !p.is_online() {
        return;
    }
    let uuid = p.get_unique_id();
    let mut lines = Vec::new();
    if conf::get().daily && daily_ready(&uuid) {
        lines.push("<#4ade80>Your daily reward is ready!</#4ade80> <click:run_command:/daily><#22d3ee><u>Claim it</u></#22d3ee></click>");
    }
    if !ready_playtime(&uuid).is_empty() {
        lines.push("<#facc15>You have playtime rewards to collect.</#facc15> <click:run_command:/rewards><#22d3ee><u>Open</u></#22d3ee></click>");
    }
    for l in &lines {
        ui::say(p, SECTION, l);
    }
    if !lines.is_empty() {
        ui::sound(p, "BLOCK_NOTE_BLOCK_PLING:0.6:1.6");
    }
}

// ---- Daily ----

fn daily_ready(uuid: &str) -> bool {
    now().saturating_sub(data::get(uuid).last_daily) >= READY_MS
}

/// The day (1-7) the next claim is, as of now.
fn next_day(d: &data::Profile) -> u32 {
    let since = now().saturating_sub(d.last_daily);
    if d.last_daily == 0 || since > KEEP_MS { 1 } else { d.daily_streak % 7 + 1 }
}

fn claim_daily(p: &Player) {
    let s = conf::get();
    let uuid = p.get_unique_id();
    let d = data::get(&uuid);
    let left = READY_MS.saturating_sub(now().saturating_sub(d.last_daily));
    if d.last_daily != 0 && left > 0 {
        ui::deny(p, &format!("Your next daily reward is ready in {}.", ui::duration(left / 1000)));
        return;
    }
    let day = next_day(&d);
    let reward = s.daily_days.get(day as usize - 1).cloned().unwrap_or_default();
    data::edit(&uuid, |d| {
        d.daily_streak = if day == 1 && (d.last_daily == 0 || now().saturating_sub(d.last_daily) > KEEP_MS) { 1 } else { d.daily_streak + 1 };
        d.last_daily = now();
        d.daily_total += 1;
    });
    conf::give(p, &reward);
    let streak = data::get(&uuid).daily_streak;
    ui::title(p, &format!("<#4ade80><bold>DAY {day}</bold>"), &format!("<#a1a1aa>daily reward claimed • streak <white>{streak}"));
    ui::fanfare(p);
    ui::say(p, SECTION, &format!("<#4ade80>Day {day} claimed!</#4ade80> {}  <#71717a>Come back tomorrow for day {}.", reward.lore().iter().map(|l| l.trim().to_owned()).collect::<Vec<_>>().join(" "), day % 7 + 1));
}

pub fn open_daily(p: &Player) {
    if !crate::allowed(p, "horizonsmp.daily") {
        return;
    }
    let s = conf::get();
    let uuid = p.get_unique_id();
    let d = data::get(&uuid);
    let ready = daily_ready(&uuid);
    let today = next_day(&d);
    let mut m = ui::frame(4, "✦ Daily Reward");
    let left = READY_MS.saturating_sub(now().saturating_sub(d.last_daily));
    m.item(4, ui::icon("CLOCK", &ui::grad("<bold>Daily Reward"), &[
        String::new(),
        format!("<#a1a1aa>Streak      <white>{} day(s)", d.daily_streak),
        format!("<#a1a1aa>Claimed     <white>{} time(s)", d.daily_total),
        if ready { "<#4ade80>Today's reward is ready!".into() } else { format!("<#a1a1aa>Next in      <white>{}", ui::duration(left / 1000)) },
        String::new(),
        "<#71717a>Miss two days and the streak".into(),
        "<#71717a>starts again from day 1.".into(),
    ]));
    for day in 1..=7u32 {
        let slot = 9 + day as usize;
        let reward = s.daily_days.get(day as usize - 1).cloned().unwrap_or_default();
        // Days before the one up next are done (in this week of the streak).
        let done = day < today || (day == today && !ready);
        let current = day == today && ready;
        let (mat, title) = if current {
            ("CHEST_MINECART", format!("<#4ade80><bold>Day {day} • Ready!"))
        } else if done {
            ("LIME_STAINED_GLASS_PANE", format!("<#4ade80><bold>Day {day} ✔"))
        } else {
            (if day == 7 { "ENDER_CHEST" } else { "CHEST" }, format!("<#a1a1aa><bold>Day {day}"))
        };
        let mut lore = vec![String::new()];
        lore.extend(reward.lore());
        lore.push(String::new());
        lore.push(if current { "<#22d3ee>▸ Click to claim".into() } else if done { "<#71717a>Claimed".into() } else { "<#71717a>Locked".into() });
        let mut icon = ui::amount(ui::icon(mat, &title, &lore), day as i32);
        if current {
            ui::glint(&mut icon);
        }
        m.button(slot, icon, move |c| {
            if current {
                claim_daily(&c.player);
                open_daily(&c.player);
            } else {
                ui::click(&c.player);
            }
        });
    }
    ui::back_button(&mut m, "the server menu", crate::menu::open);
    ui::close_button(&mut m);
    m.open(p);
}

// ---- Playtime rewards ----

fn ready_playtime(uuid: &str) -> Vec<conf::PlaytimeReward> {
    let d = data::get(uuid);
    conf::get().playtime_rewards.iter().filter(|r| d.playtime >= r.hours * 3600 && !d.playtime_claimed.contains(&r.id)).cloned().collect()
}

fn claim_playtime(p: &Player, id: &str) {
    let uuid = p.get_unique_id();
    let Some(r) = ready_playtime(&uuid).into_iter().find(|r| r.id == id) else { return };
    data::edit(&uuid, |d| d.playtime_claimed.push(r.id.clone()));
    conf::give(p, &r.reward);
    ui::title(p, &format!("<#facc15><bold>{}</bold>", r.name.to_ascii_uppercase()), &format!("<#a1a1aa>{} hour(s) played", r.hours));
    ui::fanfare(p);
    ui::say(p, SECTION, &format!("<#facc15>{}</#facc15> collected: {}", r.name, r.reward.lore().iter().map(|l| l.trim().to_owned()).collect::<Vec<_>>().join(" ")));
}

pub fn open_playtime(p: &Player) {
    if !crate::allowed(p, "horizonsmp.rewards") {
        return;
    }
    let uuid = p.get_unique_id();
    let d = data::get(&uuid);
    let list = conf::get().playtime_rewards.clone();
    let mut m = ui::frame(4, "✦ Playtime Rewards");
    m.item(4, ui::icon("CLOCK", &ui::grad("<bold>Playtime"), &[String::new(), format!("<#a1a1aa>You have played <white>{}", ui::duration(d.playtime)), String::new(), "<#71717a>Collect a reward at each milestone.".into()]));
    let slots: Vec<usize> = (10..17).chain(19..26).collect();
    for (slot, r) in slots.into_iter().zip(list) {
        let claimed = d.playtime_claimed.contains(&r.id);
        let ready = !claimed && d.playtime >= r.hours * 3600;
        let mut lore = vec![String::new(), format!("<#a1a1aa>At <white>{} hour(s)</white> played", r.hours)];
        lore.extend(r.reward.lore());
        lore.push(String::new());
        if claimed {
            lore.push("<#4ade80>✔ Collected".into());
        } else if ready {
            lore.push("<#22d3ee>▸ Click to collect".into());
        } else {
            let left = (r.hours * 3600).saturating_sub(d.playtime);
            lore.push(format!("<#71717a>{} to go", ui::duration(left)));
            lore.push(format!("<#71717a>{}", ui::meter(d.playtime as f64 / (r.hours * 3600) as f64, 20, "#facc15", "#3f3f46")));
        }
        let mat = if claimed { "LIME_STAINED_GLASS_PANE".to_owned() } else { r.icon.clone() };
        let colour = if claimed { "<#4ade80>" } else if ready { "<#facc15>" } else { "<#a1a1aa>" };
        let mut icon = ui::icon(&mat, &format!("{colour}<bold>{}", r.name), &lore);
        if ready {
            ui::glint(&mut icon);
        }
        let id = r.id.clone();
        m.button(slot, icon, move |c| {
            if ready {
                claim_playtime(&c.player, &id);
                open_playtime(&c.player);
            } else {
                ui::click(&c.player);
            }
        });
    }
    ui::back_button(&mut m, "the server menu", crate::menu::open);
    ui::close_button(&mut m);
    m.open(p);
}

// ---- Commands ----

pub fn daily_cmd(sender: &CommandSender, _args: &[&str]) -> bool {
    if let Some(p) = sender.as_player() {
        if conf::get().daily {
            open_daily(&p.clone());
        } else {
            ui::say(p, SECTION, "<#f87171>Daily rewards are turned off.");
        }
    }
    true
}

pub fn rewards_cmd(sender: &CommandSender, _args: &[&str]) -> bool {
    if let Some(p) = sender.as_player() {
        open_playtime(&p.clone());
    }
    true
}

pub fn playtime_cmd(sender: &CommandSender, args: &[&str]) -> bool {
    if !args.is_empty() && !sender.has_permission("horizonsmp.playtime.others") {
        ui::tell(sender, SECTION, "<#f87171>You don't have permission for that (horizonsmp.playtime.others).");
        return true;
    }
    let target = match args.first() {
        Some(n) => data::uuid_of(n),
        None => sender.as_player().map(|p| (p.get_unique_id(), p.get_name().to_owned())),
    };
    let Some((uuid, name)) = target else {
        ui::tell(sender, SECTION, "<#f87171>No such player.");
        return true;
    };
    if horizon_bukkit::fam_ess::get_player_by_id(&uuid).is_some() {
        count(&uuid);
    }
    let secs = data::summaries().read(|m| m.get(&uuid).map(|s| s.playtime)).unwrap_or(0).max(data::players().peek(&uuid, |d| d.playtime).unwrap_or(0));
    ui::tell(sender, SECTION, &format!("<white>{name}</white> has played for <#22d3ee>{}</#22d3ee>.", ui::duration(secs)));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn streak_days() {
        let mut d = data::Profile::default();
        assert_eq!(next_day(&d), 1);
        d.last_daily = now() - 21 * 3_600_000;
        d.daily_streak = 3;
        assert_eq!(next_day(&d), 4);
        d.daily_streak = 7;
        assert_eq!(next_day(&d), 1);
        d.daily_streak = 3;
        d.last_daily = now() - 3 * DAY_MS;
        assert_eq!(next_day(&d), 1);
    }
}
