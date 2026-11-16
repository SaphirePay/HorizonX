//! `/menu`: everything on the server one click away. Buttons for other plugins (auctions, jobs, skills,
//! backpacks, homes, warps) show only when that plugin is running, and run its command as the player. Each row
//! centres whatever it holds, so a server without some plugin still gets a tidy menu.

use crate::{bounty, claims, conf, crates, data, rewards, shop, stats, teams, ui};
use horizon_bukkit::prelude::*;
use horizon_kit::gui;
use horizon_kit::hooks::economy;
use std::sync::Arc;

fn has(plugin: &str) -> bool {
    horizon_bukkit::fam_core::get_plugin_manager().is_plugin_enabled(plugin)
}

fn run(p: &Player, cmd: &str) {
    p.close_inventory();
    let (p, cmd) = (p.clone(), cmd.to_owned());
    horizon_kit::app::later(1, move || {
        p.perform_command(&cmd);
    });
}

type Click = Arc<dyn Fn(&gui::Ctx) + Send + Sync>;

/// One button of the menu.
struct B {
    icon: ItemStack,
    click: Click,
}

fn b(icon: ItemStack, click: impl Fn(&gui::Ctx) + Send + Sync + 'static) -> B {
    B { icon, click: Arc::new(click) }
}

/// A button that runs another plugin's command.
fn cmd(mat: &str, title: &str, lore: &[&str], command: &'static str) -> B {
    let mut l = vec![String::new()];
    l.extend(lore.iter().map(|s| format!("<#a1a1aa>{s}")));
    l.push(String::new());
    l.push(format!("<#22d3ee>▸ /{command}"));
    b(ui::icon(mat, title, &l), move |c| run(&c.player, command))
}

fn info(mat: &str, title: &str, lore: &[&str], shown: &str) -> ItemStack {
    let mut l = vec![String::new()];
    l.extend(lore.iter().map(|s| format!("<#a1a1aa>{s}")));
    l.push(String::new());
    l.push(format!("<#22d3ee>▸ {shown}"));
    ui::icon(mat, title, &l)
}

fn columns(n: usize) -> Vec<usize> {
    gui::centered(n)
}

pub fn open(p: &Player) {
    if !crate::allowed(p, "horizonsmp.menu") {
        return;
    }
    let s = conf::get();
    let uuid = p.get_unique_id();
    let d = data::get(&uuid);
    let team = teams::team_name(&uuid).unwrap_or_else(|| "<#71717a>none".into());
    let daily_ready = s.daily && horizon_kit::time::now_ms().saturating_sub(d.last_daily) >= 20 * 3_600_000;
    let protected = crate::combat::protected(&uuid);

    // Money and loot.
    let mut economy_row = vec![
        b(info("EMERALD", "<#4ade80><bold>Shop", &["Buy and sell blocks, ores,", "food, mob drops and more."], "/shop"), |c| {
            ui::click(&c.player);
            shop::open(&c.player);
        }),
        b(info("HOPPER", "<#facc15><bold>Sell", &["Drop items in, get paid.", "Also: /sell hand • /sell all"], "/sell"), |c| {
            ui::click(&c.player);
            shop::open_sell(&c.player);
        }),
    ];
    if has("zAuctionHouse") || has("zAuctionHouseV3") {
        economy_row.push(cmd("GOLD_INGOT", "<#facc15><bold>Auction House", &["Buy from and sell to", "other players."], "ah"));
    }
    if has("AxDarkAuctions") {
        economy_row.push(cmd("ECHO_SHARD", "<#a855f7><bold>Dark Auction", &["Rare items, sold to the", "highest bidder."], "darkauction"));
    }
    economy_row.push(b(info("TRIPWIRE_HOOK", "<#a855f7><bold>Crates", &["Every crate's prizes and", "how many keys you have."], "/crates"), |c| {
        ui::click(&c.player);
        crates::open_menu(&c.player);
    }));

    // Rewards and rivals.
    let mut daily = ui::icon(if daily_ready { "CHEST_MINECART" } else { "MINECART" }, "<#4ade80><bold>Daily Reward", &[
        String::new(),
        if daily_ready { "<#4ade80>Ready to claim!".into() } else { "<#a1a1aa>Come back tomorrow.".into() },
        format!("<#a1a1aa>Streak <white>{} day(s)", d.daily_streak),
        String::new(),
        "<#22d3ee>▸ /daily".into(),
    ]);
    if daily_ready {
        ui::glint(&mut daily);
    }
    let reward_row = vec![
        b(daily, |c| {
            ui::click(&c.player);
            rewards::open_daily(&c.player);
        }),
        b(info("NETHER_STAR", "<#facc15><bold>Playtime Rewards", &["Rewards for the hours", "you play."], "/rewards"), |c| {
            ui::click(&c.player);
            rewards::open_playtime(&c.player);
        }),
        b(info("WITHER_SKELETON_SKULL", "<#f87171><bold>Bounties", &["The most wanted players and", "the price on their heads."], "/bounty"), |c| {
            ui::click(&c.player);
            bounty::open(&c.player);
        }),
        b(info("GOLD_BLOCK", "<#facc15><bold>Leaderboards", &["The top players."], "/top"), |c| {
            ui::click(&c.player);
            stats::open_top(&c.player, 0);
        }),
        b(
            ui::icon("SHIELD", "<#4ade80><bold>PvP Protection", &[String::new(), if protected { "<#4ade80>You're protected while new.".into() } else { "<#f87171>PvP is on for you.".into() }, String::new(), "<#22d3ee>▸ /pvp".into()]),
            |c| {
                c.close();
                crate::combat::pvp_cmd(&CommandSender::Player(c.player.clone()), &[]);
            },
        ),
    ];

    // Land, people and travel.
    let mut land_row = vec![
        b(info("GOLDEN_SHOVEL", "<#facc15><bold>Land Claims", &["Protect your builds from", "griefers and thieves."], "/claim"), |c| {
            ui::click(&c.player);
            claims::open_main(&c.player);
        }),
        b(info("PURPLE_BANNER", "<#22d3ee><bold>Team", &["Shared claims, a team home", "and team chat."], "/team"), |c| {
            ui::click(&c.player);
            teams::open(&c.player);
        }),
    ];
    if has("Essentials") {
        land_row.push(cmd("RED_BED", "<#22d3ee><bold>Homes", &["Your saved homes.", "Set one: /sethome <name>"], "homes"));
        land_row.push(cmd("ENDER_PEARL", "<#22d3ee><bold>Warps", &["The server's warps."], "warps"));
        land_row.push(cmd("COMPASS", "<#4ade80><bold>Random Teleport", &["Go somewhere new in the", "wild to start your base."], "tpr"));
        land_row.push(cmd("BEACON", "<#a855f7><bold>Spawn", &["Back to spawn."], "spawn"));
    }

    // Progress and extras.
    let mut extra_row = Vec::new();
    if has("Jobs") || has("HorizonJobs") {
        extra_row.push(cmd("IRON_PICKAXE", "<#facc15><bold>Jobs", &["Get paid for mining, farming,", "fighting and more."], "jobs browse"));
    }
    if has("mcMMO") {
        extra_row.push(cmd("DIAMOND_SWORD", "<#22d3ee><bold>Skills", &["Level up your skills as", "you play."], "mcstats"));
    }
    if has("ZenithBackpacks") {
        extra_row.push(cmd("BUNDLE", "<#a855f7><bold>Backpack", &["Extra storage you carry."], "backpack"));
    }
    extra_row.push(b(info("WRITABLE_BOOK", "<#a1a1aa><bold>Rules", &["Please read them."], "/rules"), |c| {
        c.close();
        crate::rules(&c.player);
    }));
    if has("Tebex") && !s.store.is_empty() {
        let store = s.store.clone();
        extra_row.push(b(ui::icon("DIAMOND", "<#22d3ee><bold>Store", &[String::new(), "<#a1a1aa>Support the server.".into(), format!("<#71717a>{store}"), String::new(), "<#22d3ee>▸ /buy".into()]), |c| run(&c.player, "buy")));
    }

    let mut m = ui::frame(6, &format!("✦ {}", s.server));
    let mut head = ui::head(p.get_name(), &ui::grad(&format!("<bold>{}</bold>", p.get_name())), &[
        String::new(),
        format!("<#a1a1aa>Balance    <#4ade80>{}", ui::money(economy::balance(p.get_name()))),
        format!("<#a1a1aa>Playtime  <white>{}", ui::duration(d.playtime)),
        format!("<#a1a1aa>Team        <white>{team}"),
        format!("<#a1a1aa>Claim blocks <white>{}", ui::num(claims::total_blocks(&uuid) - claims::used_blocks(&uuid))),
        String::new(),
        "<#22d3ee>▸ Click for your stats".into(),
    ]);
    ui::glint(&mut head);
    m.button(4, head, |c| {
        ui::click(&c.player);
        stats::open(&c.player, None);
    });
    for (row, buttons) in [(1usize, economy_row), (2, reward_row), (3, land_row), (4, extra_row)] {
        for (col, button) in columns(buttons.len()).into_iter().zip(buttons) {
            let click = button.click.clone();
            m.button(row * 9 + col, button.icon, move |c| click(c));
        }
    }
    ui::close_button(&mut m);
    m.open(p);
}

pub fn menu_cmd(sender: &CommandSender, _args: &[&str]) -> bool {
    match sender.as_player() {
        Some(p) => open(&p.clone()),
        None => ui::tell(sender, "Menu", "Only players have a menu."),
    }
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn rows_are_centred() {
        for n in 0..=7 {
            let c = super::columns(n);
            assert_eq!(c.len(), n);
            // Mirror-symmetric around column 4.
            let mirrored: Vec<usize> = c.iter().rev().map(|x| 8 - x).collect();
            assert_eq!(c, mirrored, "{n}");
        }
    }
}
