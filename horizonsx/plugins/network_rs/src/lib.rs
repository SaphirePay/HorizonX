//! Network: moving players between the servers of a HorizonX network (horizon-proxy in front of them).
//!
//! `/server` lists the network's servers with their players (click one to go), `/server <name>` goes there, and
//! `/hub` (also `/lobby`) goes back to the lobby. A move tells the proxy where the player's next login goes (its
//! control port: `<secret> SEND <uuid> <server>`), then sends the player the game's Transfer packet back to the
//! network's address, and the proxy routes that login.
//!
//! HorizonX writes `plugins/Network/config.yml` when it puts a server in a network:
//! server (this server's name in the proxy), lobby, address and port (where players join the network),
//! control_port (the proxy's control port on 127.0.0.1) and secret (the network's forwarding secret).
use horizon_bukkit::{CommandSender, Player, Plugin, bukkit_plugin, mini_message};
use horizon_kit::app;
use horizon_kit::config::Config;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

#[derive(Clone, Default)]
struct Settings {
    server: String,
    lobby: String,
    address: String,
    port: i32,
    control_port: u16,
    secret: String,
}

fn settings() -> Option<Settings> {
    let text = std::fs::read_to_string(app::folder().join("config.yml")).ok()?;
    let c = Config::parse(&text);
    let s = Settings {
        server: c.str("server", ""),
        lobby: c.str("lobby", ""),
        address: c.str("address", ""),
        port: c.int("port", 25577) as i32,
        control_port: c.int("control_port", 25600) as u16,
        secret: c.str("secret", ""),
    };
    (!s.secret.is_empty() && !s.address.is_empty()).then_some(s)
}

/// One line to the proxy's control port and its answer.
fn ask(s: &Settings, what: &str) -> Result<String, String> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], s.control_port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(500)).map_err(|_| "The network proxy isn't running.".to_owned())?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).map_err(|e| e.to_string())?;
    stream.write_all(format!("{} {what}\n", s.secret).as_bytes()).map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).map_err(|e| e.to_string())?;
    Ok(line.trim().to_owned())
}

fn say(p: &Player, text: &str) {
    p.send_component(&mini_message(&format!("<#a855f7><bold>Network</bold> <#52525b>» <white>{text}")));
}

fn reply(sender: &CommandSender, text: &str) {
    sender.send_message(&horizon_kit::mini::legacy(&format!("<#a855f7><bold>Network</bold> <#52525b>» <white>{text}")));
}

/// The network's servers and their players, from the proxy.
fn servers(s: &Settings) -> Result<Vec<(String, u64)>, String> {
    let answer = ask(s, "COUNTS")?;
    let v: serde_json::Value = serde_json::from_str(&answer).map_err(|_| format!("The proxy answered: {answer}"))?;
    let mut list: Vec<(String, u64)> = v["servers"].as_object().map(|o| o.iter().map(|(k, n)| (k.clone(), n.as_u64().unwrap_or(0))).collect()).unwrap_or_default();
    // The lobby first, then by name.
    list.sort_by_key(|(n, _)| (n != &s.lobby, n.clone()));
    Ok(list)
}

fn go(p: &Player, s: &Settings, server: &str) {
    if server.eq_ignore_ascii_case(&s.server) {
        say(p, &format!("You're already on <#c4b5fd>{server}</#c4b5fd>."));
        return;
    }
    match ask(s, &format!("SEND {} {server}", p.get_unique_id())) {
        Ok(a) if a == "OK" => {
            say(p, &format!("Sending you to <#c4b5fd>{server}</#c4b5fd>..."));
            p.transfer(&s.address, s.port);
        }
        Ok(a) if a.starts_with("ERR no server") => say(p, &format!("<#fca5a5>There's no server called {server}. <white>Type <#c4b5fd>/server</#c4b5fd> to see them.")),
        Ok(a) if a.starts_with("QUEUED") => say(p, &format!("You're in the line for <#c4b5fd>{server}</#c4b5fd>.")),
        Ok(a) => say(p, &format!("<#fca5a5>The proxy said: {a}")),
        Err(e) => say(p, &format!("<#fca5a5>{e}")),
    }
}

fn list(p: &Player, s: &Settings) {
    match servers(s) {
        Ok(list) => {
            say(p, "Servers on this network <#71717a>(click one to go)");
            for (name, n) in list {
                let here = name.eq_ignore_ascii_case(&s.server);
                let tag = if name == s.lobby { " <#71717a>lobby" } else { "" };
                let line = if here {
                    format!(" <#4ade80>● <white><bold>{name}</bold>{tag} <#71717a>· {n} online · you're here")
                } else {
                    format!(" <click:run_command:'/server {name}'><hover:show_text:'<white>Go to {name}'><#a855f7>▸ <white>{name}{tag} <#71717a>· {n} online</hover></click>")
                };
                p.send_component(&mini_message(&line));
            }
        }
        Err(e) => say(p, &format!("<#fca5a5>{e}")),
    }
}

fn on_command(sender: &CommandSender, label: &str, args: &[&str]) -> bool {
    let Some(p) = sender.as_player() else {
        reply(sender, "Players only.");
        return true;
    };
    let Some(s) = settings() else {
        say(p, "<#fca5a5>This server isn't in a network yet. <white>Add it on HorizonX's Network page.");
        return true;
    };
    match (label.to_ascii_lowercase().as_str(), args.first()) {
        ("hub" | "lobby", _) => {
            if s.lobby.is_empty() {
                say(p, "<#fca5a5>The network has no lobby set.");
            } else {
                go(p, &s, &s.lobby.clone());
            }
        }
        (_, Some(server)) => go(p, &s, server),
        (_, None) => list(p, &s),
    }
    true
}

fn on_enable(plugin: Plugin) {
    app::set(plugin);
    plugin.set_executor("server", on_command);
    plugin.set_executor("hub", on_command);
    plugin.set_tab_completer("server", |_, _, args| {
        if args.len() > 1 {
            return Vec::new();
        }
        let Some(s) = settings() else { return Vec::new() };
        let typed = args.first().map(|a| a.to_ascii_lowercase()).unwrap_or_default();
        servers(&s).map(|l| l.into_iter().map(|(n, _)| n).filter(|n| n.starts_with(&typed)).collect()).unwrap_or_default()
    });
    match settings() {
        Some(s) => app::info(&format!("Network ready: this is {} (lobby {}), players join at {}:{}", s.server, s.lobby, s.address, s.port)),
        None => app::info("Network: not in a network yet (HorizonX writes plugins/Network/config.yml)"),
    }
}

fn on_disable(_plugin: Plugin) {
    app::clear();
}

bukkit_plugin!(on_enable, on_disable);
