//! `/axdarkauctions` (aliases from `command-aliases`: axda, da, darkauctions, darkauction) and its subcommands, with
//! the Lamp messages of lang.yml `commands.*`.

use crate::auction::{self, State};
use crate::{send_lang, texts};
use horizon_bukkit::{CommandSender, Player, Plugin};

struct Sub {
    name: &'static str,
    perm: &'static str,
    /// Its arguments as Lamp names them (`<auction>` required, `[auction]` optional).
    args: &'static [&'static str],
    player: bool,
}

const SUBS: &[Sub] = &[
    Sub { name: "help", perm: "axdarkauctions.use", args: &[], player: false },
    Sub { name: "create", perm: "axdarkauctions.create", args: &["<name>"], player: false },
    Sub { name: "delete", perm: "axdarkauctions.delete", args: &["<auction>"], player: false },
    Sub { name: "editor", perm: "axdarkauctions.editor", args: &["<auction>"], player: true },
    Sub { name: "schedulers", perm: "axdarkauctions.schedulers", args: &[], player: true },
    Sub { name: "start", perm: "axdarkauctions.start", args: &["<auction>"], player: false },
    Sub { name: "stop", perm: "axdarkauctions.stop", args: &["<auction>"], player: false },
    Sub { name: "stopall", perm: "axdarkauctions.stopall", args: &[], player: false },
    Sub { name: "join", perm: "axdarkauctions.join", args: &["[auction]"], player: true },
    Sub { name: "leave", perm: "axdarkauctions.leave", args: &[], player: true },
    Sub { name: "info", perm: "axdarkauctions.info", args: &["[auction]"], player: false },
    Sub { name: "forcejoin", perm: "axdarkauctions.forcejoin", args: &["<auction>", "<player>"], player: false },
    Sub { name: "view", perm: "axdarkauctions.view", args: &["[auction]"], player: true },
    Sub { name: "reload", perm: "axdarkauctions.reload", args: &[], player: false },
];

fn lamp(sender: &CommandSender, key: &str, reps: &[(&str, &str)]) {
    send_lang(sender, &format!("commands.{key}"), reps);
}

pub fn register(plugin: &Plugin) {
    for name in ["axdarkauctions"] {
        plugin.set_executor(name, |sender, _label, args| {
            run(sender, args);
            crate::finish();
            true
        });
        plugin.set_tab_completer(name, |sender, _label, args| complete(sender, args));
    }
}

fn run(sender: &CommandSender, args: &[&str]) {
    if !sender.has_permission("axdarkauctions.use") {
        return lamp(sender, "no-permission", &[]);
    }
    let Some(first) = args.first() else { return help(sender) };
    let Some(sub) = SUBS.iter().find(|s| s.name.eq_ignore_ascii_case(first)) else {
        return lamp(sender, "invalid-command", &[]);
    };
    if !sender.has_permission(sub.perm) {
        return lamp(sender, "no-permission", &[]);
    }
    let player = match sender {
        CommandSender::Player(p) => Some(p.clone()),
        _ => None,
    };
    if sub.player && player.is_none() {
        return lamp(sender, "player-only", &[]);
    }
    let rest = &args[1..];
    for (i, a) in sub.args.iter().enumerate() {
        if a.starts_with('<') && rest.get(i).is_none() {
            return lamp(sender, "missing-argument", &[("%value%", a.trim_matches(['<', '>']))]);
        }
    }
    // An `Auction` argument must name one (Lamp's resolver).
    let mut auction_arg = None;
    if let Some(at) = sub.args.iter().position(|a| a.contains("auction"))
        && let Some(given) = rest.get(at)
    {
        if !auction::exists(given) {
            return send_lang(sender, "errors.not-found", &[]);
        }
        auction_arg = Some((*given).to_owned());
    }
    match sub.name {
        "help" => help(sender),
        "create" => {
            let name = rest[0];
            if auction::create(name) {
                send_lang(sender, "created", &[("%auction%", name)]);
            } else {
                send_lang(sender, "errors.already-exists", &[("%auction%", name)]);
            }
        }
        "delete" => {
            let name = auction_arg.unwrap_or_default();
            auction::delete(&name);
            send_lang(sender, "deleted", &[("%auction%", &name)]);
        }
        "editor" => {
            if let (Some(p), Some(name)) = (player, auction_arg) {
                crate::editor::main(&p, &name);
            }
        }
        "schedulers" => {
            if let Some(p) = player {
                crate::editor::schedulers(&p);
            }
        }
        "start" => {
            let name = auction_arg.unwrap_or_default();
            if auction::with(&name, |a| a.running).unwrap_or(false) {
                send_lang(sender, "errors.already-running", &[]);
            } else {
                auction::with(&name, |a| a.start());
                send_lang(sender, "start", &[("%auction%", &name)]);
            }
        }
        "stop" => {
            let name = auction_arg.unwrap_or_default();
            if !auction::with(&name, |a| a.running).unwrap_or(false) {
                send_lang(sender, "errors.not-running", &[]);
            } else {
                auction::with(&name, |a| a.stop());
                send_lang(sender, "stop", &[("%auction%", &name)]);
            }
        }
        "stopall" => {
            let stopped = auction::stop_all();
            send_lang(sender, "stop", &[("%auction%", &stopped.join(", "))]);
        }
        "join" => {
            if let Some(p) = player {
                join(&p, auction_arg);
            }
        }
        "leave" => {
            if let Some(p) = player {
                leave(&p);
            }
        }
        "info" => info(sender, auction_arg),
        "forcejoin" => {
            let name = auction_arg.unwrap_or_default();
            let Some(target) = horizon_bukkit::get_player(rest[1]) else {
                return lamp(sender, "invalid-player", &[("%player%", rest[1])]);
            };
            if !auction::with(&name, |a| a.running).unwrap_or(false) {
                crate::tell_lang(&target, "errors.not-running", &[]);
            } else if auction::join(&name, &target, false) {
                let fancy = auction::with(&name, |a| a.fancy.clone()).unwrap_or_default();
                send_lang(sender, "joined", &[("%auction%", &fancy)]);
            }
        }
        "view" => {
            if let Some(p) = player {
                view(&p, auction_arg);
            }
        }
        "reload" => reload(sender),
        _ => {}
    }
}

fn help(sender: &CommandSender) {
    let key = if sender.has_permission("axdarkauctions.admin") { "admin-help" } else { "player-help" };
    for line in crate::lang_list(key) {
        texts::send_to(sender, &line);
    }
}

/// Join: the named auction, or any running one.
pub fn join(p: &Player, name: Option<String>) {
    let sender = CommandSender::Player(p.clone());
    if auction::in_any(p) {
        return send_lang(&sender, "errors.already-in", &[]);
    }
    let name = name.or_else(auction::any_running);
    let running = name.as_ref().and_then(|n| auction::with(n, |a| a.running)).unwrap_or(false);
    let Some(name) = name.filter(|_| running) else {
        return send_lang(&sender, "errors.not-running", &[]);
    };
    if auction::join(&name, p, false) {
        let fancy = auction::with(&name, |a| a.fancy.clone()).unwrap_or_default();
        send_lang(&sender, "joined", &[("%auction%", &fancy)]);
    }
}

pub fn leave(p: &Player) {
    let sender = CommandSender::Player(p.clone());
    match auction::of_player(p) {
        Some(name) => {
            auction::leave(&name, p);
            let fancy = auction::with(&name, |a| a.fancy.clone()).unwrap_or_default();
            send_lang(&sender, "left", &[("%auction%", &fancy)]);
        }
        None => send_lang(&sender, "errors.not-in", &[]),
    }
}

fn info(sender: &CommandSender, name: Option<String>) {
    let Some(name) = name.or_else(|| auction::names().into_iter().next()) else {
        return send_lang(sender, "errors.not-found", &[]);
    };
    let shown = auction::next_run_in(&name).map_or_else(|| "---".to_owned(), crate::format_time);
    send_lang(sender, "info", &[("%time%", &shown)]);
}

fn view(p: &Player, name: Option<String>) {
    let sender = CommandSender::Player(p.clone());
    let name = name.or_else(auction::any_running);
    let state = name.as_ref().and_then(|n| auction::with(n, |a| (a.running, a.state, a.has(p), a.banned.contains(&p.get_unique_id()))));
    let Some((true, state, inside, banned)) = state else {
        return send_lang(&sender, "errors.not-running", &[]);
    };
    if !inside || banned {
        return send_lang(&sender, "errors.not-in", &[]);
    }
    let name = name.unwrap_or_default();
    match state {
        State::Bidding => crate::menus::open(p, &name, crate::menus::Kind::Bidding),
        State::Showing => crate::menus::open(p, &name, crate::menus::Kind::Preview),
        _ => {}
    }
}

fn reload(sender: &CommandSender) {
    texts::console("&#D200FF[AxDarkAuctions] &#FF00FFReloading configuration...");
    crate::reload_files();
    for file in ["config.yml", "lang.yml", "currencies.yml", "schedulers.yml", "guis/main.yml", "guis/preview.yml"] {
        texts::console(&format!("&#D200FF╠ &#FF00FFReloaded &f{file}&#77BBFF!"));
    }
    texts::console("&#D200FF╚ &#FF00FFSuccessful reload!");
    send_lang(sender, "reload.success", &[]);
}

fn complete(sender: &CommandSender, args: &[&str]) -> Vec<String> {
    let starts = |options: Vec<String>, typed: &str| options.into_iter().filter(|o| o.to_ascii_lowercase().starts_with(&typed.to_ascii_lowercase())).collect::<Vec<_>>();
    match args.len() {
        0 | 1 => starts(SUBS.iter().filter(|s| sender.has_permission(s.perm)).map(|s| s.name.to_owned()).collect(), args.first().copied().unwrap_or("")),
        n => {
            let Some(sub) = SUBS.iter().find(|s| s.name.eq_ignore_ascii_case(args[0])) else { return Vec::new() };
            let Some(kind) = sub.args.get(n - 2) else { return Vec::new() };
            let typed = args[n - 1];
            if kind.contains("auction") {
                starts(auction::names(), typed)
            } else if kind.contains("player") {
                starts(horizon_bukkit::get_online_players().iter().map(|p| p.get_name().to_owned()).collect(), typed)
            } else {
                Vec::new()
            }
        }
    }
}
