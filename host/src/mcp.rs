//! `HorizonX mcp`: HorizonX as an MCP server, so an AI assistant (Claude, Cursor, any MCP client) can do what
//! a person does in the app: make and run servers, change their settings and files, add plugins (the ones the
//! app ships, any from Modrinth with what they need, or jars), run the network, moderate players, back up and
//! restore, and diagnose problems. Every tool calls the same library functions the app's buttons call, so what
//! an agent does shows in the open app and the other way round.
//!
//! JSON-RPC over stdin and stdout (the shared `horizon-mcp` crate does the protocol). Tools that delete or
//! overwrite something the person can't get back ask for `confirm: true`.
use horizon_launcher::{host, network, plugin_market, servers};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

const INSTRUCTIONS: &str = "HorizonX hosts Minecraft Java Edition 26.3 servers on HorizonsX (a Rust server engine) from this PC, \
with a public address through playit.gg and an optional proxy network joining several servers. Start with \
horizonx_overview (every server, its state and the network). `server` arguments take a server's name or folder. \
diagnose finds problems (TPS, crashes, log errors, failing plugins, the public address, updates) and says what to do. \
Plugins: plugin_catalog = the ones HorizonX ships; modrinth_search + modrinth_install = any Paper/Spigot/Bukkit plugin \
for 26.3 from Modrinth with its required dependencies (Java plugins run in HorizonsX's Java layer, added for them). \
Settings: server_settings shows server.toml; server_settings_set changes keys (checked first). Plugin configs: \
server_files / server_file_read / server_file_write. Deleting, restoring a backup or removing a plugin needs \
confirm: true, only after the person agreed. Creating a server needs accept_eula: true, only with the person's \
agreement to the Minecraft EULA (https://aka.ms/MinecraftEULA).";

fn server() -> horizon_mcp::Server<'static> {
    horizon_mcp::Server::new("horizonx", env!("CARGO_PKG_VERSION"), INSTRUCTIONS)
}

/// The servers and the proxy this MCP server starts outlive it. On Windows a child gets every inheritable handle of
/// its parent, and this process's stdin and stdout are the assistant's pipes: a server holding them keeps the pipe
/// open after this process ends, so the assistant (or anything reading to the end) waits forever. Not inheritable.
#[cfg(windows)]
fn keep_stdio_to_self() {
    unsafe extern "system" {
        fn GetStdHandle(which: u32) -> isize;
        fn SetHandleInformation(handle: isize, mask: u32, flags: u32) -> i32;
    }
    const HANDLE_FLAG_INHERIT: u32 = 1;
    // STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE: -10, -11, -12.
    for which in [-10i32, -11, -12] {
        // SAFETY: plain Win32 calls on this process's own standard handles; a missing one is skipped.
        unsafe {
            let h = GetStdHandle(which as u32);
            if h != 0 && h != -1 {
                SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

#[cfg(not(windows))]
fn keep_stdio_to_self() {}

pub fn serve() -> Result<(), String> {
    keep_stdio_to_self();
    server().serve(|method, params| match method {
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).ok_or((-32602, "name is required".to_string()))?;
            if !tools().iter().any(|t| t["name"] == name) {
                return Err((-32602, format!("Unknown tool {name}")));
            }
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let (text, error) = match call(name, &args) {
                Ok(v) => (if let Value::String(s) = v { s } else { serde_json::to_string_pretty(&v).unwrap_or_default() }, false),
                Err(e) => (e, true),
            };
            Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": error }))
        }
        _ => Err((-32601, format!("Method not found: {method}"))),
    })
}

// ----- The tools -------------------------------------------------------------------------------------

fn tool(name: &str, description: &str, props: Value, required: &[&str]) -> Value {
    json!({ "name": name, "description": description, "inputSchema": { "type": "object", "properties": props, "required": required } })
}

fn srv() -> Value {
    json!({ "type": "string", "description": "The server's name or folder (horizonx_overview lists them)." })
}

fn confirm() -> Value {
    json!({ "type": "boolean", "description": "true only after the person agreed: this can't be undone from here." })
}

pub fn tools() -> Vec<Value> {
    let s = srv;
    let none = json!({});
    vec![
        // Overview and diagnostics
        tool("horizonx_overview", "Everything at a glance: each server (state, players, TPS, RAM, CPU, public address) and the network. Start here.", none.clone(), &[]),
        tool("server_status", "One server in detail: state, uptime, TPS and MSPT, players, RAM, CPU, crashes, release and update, public address (playit), schedules, plugins.", json!({ "server": s() }), &["server"]),
        tool("diagnose", "Health check with findings in plain words and what to do about each: stopped or crashed, low TPS / high MSPT, errors and failing plugins in the log, the public address, updates, the network. No server = every server and the network.", json!({ "server": s() }), &[]),
        tool("server_logs", "The last lines of a server's log (running or not). contains= keeps only lines with that text (case-insensitive), for example \"error\" or a plugin's name.", json!({ "server": s(), "lines": { "type": "integer", "description": "How many (default 80, at most 2000)." }, "contains": { "type": "string" } }), &["server"]),
        tool("server_report", "Record a performance capture of a running server for some seconds (no player data): where it was saved, to find what makes it lag.", json!({ "server": s(), "seconds": { "type": "integer", "description": "Default 30, at most 300." } }), &["server"]),
        // Servers
        tool("server_create", "Make a new server as the app's wizard does: the world, HorizonsX, the public-address plugin (playit) and chosen plugins, then (by default) start it. Needs accept_eula: true with the person's agreement to the Minecraft EULA.", json!({
            "name": { "type": "string" },
            "accept_eula": { "type": "boolean", "description": "The person agrees to the Minecraft EULA (https://aka.ms/MinecraftEULA)." },
            "game_mode": { "type": "string", "enum": ["survival", "creative", "adventure", "spectator"] },
            "difficulty": { "type": "string", "enum": ["peaceful", "easy", "normal", "hard"] },
            "max_players": { "type": "integer" },
            "seed": { "type": "string" },
            "copy_world": { "type": "string", "description": "Start from a copy of this world folder (one of the person's saves) instead of a new world." },
            "open": { "type": "boolean", "description": "Anyone may join (no whitelist). Default false: only listed players." },
            "offline": { "type": "boolean", "description": "Don't check Minecraft accounts (anyone can use any name). Default false; a security risk." },
            "players": { "type": "array", "items": { "type": "string" }, "description": "Who may join; the first becomes an operator." },
            "region_threads": { "type": "boolean", "description": "HorizonsX region threads (a TPS per region; default true). false = one clock like vanilla." },
            "view_distance": { "type": "integer", "description": "Default 10." },
            "simulation_distance": { "type": "integer", "description": "Default 8." },
            "backups": { "type": "boolean", "description": "A backup every 6 hours, keeping 3 (default true)." },
            "plugins": { "type": "array", "items": { "type": "string" }, "description": "Plugins HorizonX ships to add (plugin_catalog: file or name)." },
            "start": { "type": "boolean", "description": "Start it once made (default true)." }
        }), &["name", "accept_eula"]),
        tool("server_start", "Start a server (in the background; it keeps running after this session). Waits until it is up (wait=false to return at once).", json!({ "server": s(), "wait": { "type": "boolean" } }), &["server"]),
        tool("server_stop", "Stop a server cleanly (it saves the world).", json!({ "server": s() }), &["server"]),
        tool("server_restart", "Restart a server (players are told; the world is saved).", json!({ "server": s() }), &["server"]),
        tool("server_delete", "Delete a server: its folder goes to the trash and it leaves the list (and the network). Stop it first.", json!({ "server": s(), "confirm": confirm() }), &["server", "confirm"]),
        tool("server_command", "Run a console command on a running server (say, give, time set, gamerule, a plugin's command...) and return what it answered.", json!({ "server": s(), "command": { "type": "string" } }), &["server", "command"]),
        tool("server_settings", "A server's settings (server.toml, every key) as JSON: world, port, players, whitelist, engine, backups, panel...", json!({ "server": s() }), &["server"]),
        tool("server_settings_set", "Change server.toml settings, checked before anything is written. Keys as server_settings shows them, dotted: {\"world.difficulty\": \"hard\", \"server.max_players\": 20, \"server.view_distance\": 12, \"properties.horizonsx-region-threads\": false}. Says which apply now and which at the next restart.", json!({ "server": s(), "changes": { "type": "object", "description": "key -> new value" } }), &["server", "changes"]),
        tool("server_files", "List a folder inside a server (\"\" = its top; plugins, plugins/<Plugin>, config...). Says which files can be read and edited as text.", json!({ "server": s(), "path": { "type": "string" } }), &["server"]),
        tool("server_file_read", "Read a text file inside a server (a plugin's config.yml, server.toml, whitelist.json...), up to 1 MB.", json!({ "server": s(), "path": { "type": "string" } }), &["server", "path"]),
        tool("server_file_write", "Write a text file inside a server (yml, yaml, json, toml, properties, txt, conf, cfg). JSON is checked first; the old contents are kept in .horizon/edits/. Plugins read their configs at start or on their reload command.", json!({ "server": s(), "path": { "type": "string" }, "text": { "type": "string" } }), &["server", "path", "text"]),
        tool("server_schedule", "A server's schedules: a backup every N hours, a daily restart at an hour, or a restart every N hours of uptime. With no other argument it only reads them; null/0 turns one off.", json!({ "server": s(), "backup_hours": { "type": "integer" }, "restart_hour": { "type": ["integer", "null"], "description": "0-23 local time, null = off." }, "restart_every": { "type": "integer", "description": "Hours of uptime; 0 = off. Used instead of restart_hour." } }), &["server"]),
        tool("server_backup", "Back up a server now (world and settings; a running server saves first and stays up).", json!({ "server": s(), "name": { "type": "string" } }), &["server"]),
        tool("server_backups", "A server's backups (name, size, when).", json!({ "server": s() }), &["server"]),
        tool("server_restore", "Put a backup's world in place (the current world is kept as a backup too); a running server restarts with it.", json!({ "server": s(), "backup": { "type": "string" }, "dimension": { "type": "string", "description": "Only this dimension (minecraft:the_nether...)." }, "confirm": confirm() }), &["server", "backup", "confirm"]),
        tool("server_update", "A server's engine release: check=true (default) says whether a newer one exists; check=false installs it (backs up, restarts, goes back by itself if it fails).", json!({ "server": s(), "check": { "type": "boolean" } }), &["server"]),
        tool("server_panel_link", "A one-time login link for a running server's web panel.", json!({ "server": s() }), &["server"]),
        // Players
        tool("players", "Who is online (with ping), the operators, the whitelist and the bans.", json!({ "server": s() }), &["server"]),
        tool("player_action", "Moderate: op, deop, kick, ban, pardon, ban_ip, pardon_ip, whitelist_add, whitelist_remove (player needed), whitelist_on, whitelist_off. Works on a running server, or edits the files of a stopped one.", json!({ "server": s(), "action": { "type": "string", "enum": ["op", "deop", "kick", "ban", "pardon", "ban_ip", "pardon_ip", "whitelist_add", "whitelist_remove", "whitelist_on", "whitelist_off"] }, "player": { "type": "string", "description": "A name (an IP address for ban_ip/pardon_ip also works)." }, "reason": { "type": "string" } }), &["server", "action"]),
        // Plugins
        tool("plugin_catalog", "The plugins HorizonX ships (file, name, what it does): add them with plugin_add.", none.clone(), &[]),
        tool("plugins_list", "A server's plugins: HorizonsX plugins and Java jars (on or off), what each Java plugin is (version, authors, its config files) and which came from Modrinth.", json!({ "server": s() }), &["server"]),
        tool("plugin_add", "Add a plugin HorizonX ships to one server, several, or all (\"all\"). A running HorizonsX server loads it at once.", json!({ "plugin": { "type": "string", "description": "File or name from plugin_catalog." }, "servers": { "type": "array", "items": { "type": "string" }, "description": "Server names/folders, or [\"all\"]." } }), &["plugin", "servers"]),
        tool("plugin_remove", "Take a plugin file out of a server (its data folder stays). Stop the server first for a Java jar.", json!({ "server": s(), "plugin": { "type": "string", "description": "Its file name (plugins_list)." }, "confirm": confirm() }), &["server", "plugin", "confirm"]),
        tool("plugin_set", "Turn a plugin on or off (it stays installed; takes effect at the next restart). Also \"playit\" (the public address) and \"java layer\" (Paper/Spigot jars): remembered per server.", json!({ "server": s(), "plugin": { "type": "string" }, "on": { "type": "boolean" } }), &["server", "plugin", "on"]),
        tool("plugin_add_jar", "Copy Java plugin jars from this PC into a server (they load at its next start; the Java layer is added when needed).", json!({ "server": s(), "paths": { "type": "array", "items": { "type": "string" } } }), &["server", "paths"]),
        tool("modrinth_search", "Search Modrinth for server plugins made for Minecraft 26.3 (Paper, Spigot, Bukkit, Purpur, Folia), most downloaded first.", json!({ "query": { "type": "string" }, "offset": { "type": "integer" } }), &[]),
        tool("modrinth_install", "Install a Modrinth plugin (its id or slug, from modrinth_search) and its required dependencies into one server, several, or all; checked against Modrinth's hashes. Restart the server to load it.", json!({ "project": { "type": "string" }, "servers": { "type": "array", "items": { "type": "string" }, "description": "Server names/folders, or [\"all\"]." } }), &["project", "servers"]),
        // Network
        tool("network_status", "The proxy network: its address and port, the servers in it, the lobby, whether the proxy runs, players per server, RAM.", none.clone(), &[]),
        tool("network_create", "Join servers into one network behind the proxy (players join one address and move with /server, /hub). Sets each server up for it (forwarding secret, offline mode behind the proxy, the network plugin).", json!({ "name": { "type": "string" }, "servers": { "type": "array", "items": { "type": "string" } }, "lobby": { "type": "string", "description": "Where players land first (default: a server named lobby or hub, else the first)." }, "port": { "type": "integer", "description": "Default 25565." }, "start": { "type": "boolean", "description": "Start the proxy (default true)." } }), &["servers"]),
        tool("network_settings", "Change the network: name, port, MOTD, account checking, the lobby, the public address. The proxy restarts to take them if it runs.", json!({ "name": { "type": "string" }, "port": { "type": "integer" }, "motd": { "type": "string" }, "online_mode": { "type": "boolean" }, "lobby": { "type": "string" }, "address": { "type": "string" } }), &[]),
        tool("network_add_server", "Add a server to the network.", json!({ "server": s() }), &["server"]),
        tool("network_remove_server", "Take a server out of the network.", json!({ "server": s() }), &["server"]),
        tool("network_start", "Start the network's proxy so players can join through its address (the servers in it should be running).", none.clone(), &[]),
        tool("network_stop", "Stop the network's proxy: players on it are disconnected; the servers themselves keep running.", none.clone(), &[]),
        tool("network_restart", "Restart the network's proxy so it reads its settings again (players on it reconnect).", none.clone(), &[]),
        tool("network_log", "The network proxy's last log lines (who joined, which server they went to, errors).", json!({ "lines": { "type": "integer" } }), &[]),
        tool("network_delete", "Remove the network (the servers stay; their network settings are left as they are).", json!({ "confirm": confirm() }), &["confirm"]),
        // Setup
        tool("horizonx_setup", "Get this PC ready to host (once): install the newest Horizon release and prepare Minecraft 26.3's files from the person's own Minecraft. Takes a few minutes the first time; server_create does it too.", none, &[]),
    ]
}

// ----- Arguments ---------------------------------------------------------------------------------------

fn s<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty())
}

fn need<'a>(a: &'a Value, k: &str) -> Result<&'a str, String> {
    s(a, k).ok_or_else(|| format!("{k} is required"))
}

fn b(a: &Value, k: &str, default: bool) -> bool {
    a.get(k).and_then(Value::as_bool).unwrap_or(default)
}

fn n(a: &Value, k: &str) -> Option<u64> {
    a.get(k).and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
}

fn list(a: &Value, k: &str) -> Vec<String> {
    match a.get(k) {
        Some(Value::Array(v)) => v.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => vec![],
    }
}

fn confirmed(a: &Value, what: &str) -> Result<(), String> {
    if b(a, "confirm", false) {
        Ok(())
    } else {
        Err(format!("{what} can't be undone from here: ask the person, then call again with confirm: true."))
    }
}

/// A server of the list by name (any case) or folder.
fn find(which: &str) -> Result<servers::Entry, String> {
    let all = servers::list();
    all.iter()
        .find(|e| e.name == which)
        .or_else(|| all.iter().find(|e| e.name.eq_ignore_ascii_case(which)))
        .or_else(|| all.iter().find(|e| e.dir == Path::new(which) || e.dir.file_name().is_some_and(|f| f.eq_ignore_ascii_case(which))))
        .cloned()
        .ok_or_else(|| {
            let names: Vec<_> = all.iter().map(|e| e.name.clone()).collect();
            format!("No server called {which}. The servers: {}", if names.is_empty() { "none yet".into() } else { names.join(", ") })
        })
}

/// A server on this PC (files and plugins are only reachable for those).
fn local(a: &Value) -> Result<(servers::Entry, PathBuf), String> {
    let e = find(need(a, "server")?)?;
    if let Some(h) = &e.host {
        return Err(format!("{} runs on another computer ({h}): only start, stop, status and commands reach it from here.", e.name));
    }
    let d = e.dir.clone();
    Ok((e, d))
}

/// Many servers by name, or every one for ["all"].
fn many(a: &Value) -> Result<Vec<servers::Entry>, String> {
    let wanted = list(a, "servers");
    if wanted.is_empty() {
        return Err("servers is required (names, or [\"all\"])".into());
    }
    if wanted.iter().any(|w| w.eq_ignore_ascii_case("all")) {
        return Ok(servers::list().into_iter().filter(|e| e.host.is_none()).collect());
    }
    wanted.iter().map(|w| find(w)).collect()
}

/// One `horizon-server` command on a server, here or over SSH: what it printed, or the problem in plain words.
fn hs(e: &servers::Entry, args: &[&str]) -> Result<String, String> {
    if e.host.is_none() {
        return servers::run(&e.dir, args).map_err(|p| p.message);
    }
    let out = e.command(args)?.stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if out.status.success() { Ok(text) } else { Err(text.trim().to_string()) }
}

fn hs_json(e: &servers::Entry, args: &[&str]) -> Result<Value, String> {
    let out = hs(e, args)?;
    serde_json::from_str(out.trim()).map_err(|_| out.trim().to_string())
}

// ----- Status and diagnostics ----------------------------------------------------------------------------

fn status_json(st: &servers::Status) -> Value {
    json!({
        "phase": st.phase,
        "uptime_s": st.uptime,
        "players_online": st.players.iter().map(|p| json!({ "name": p.name, "ping_ms": p.ping })).collect::<Vec<_>>(),
        "max_players": st.max_players,
        "tps": st.tps.map(|t| (t * 10.0).round() / 10.0),
        "mspt": st.mspt.map(|t| (t * 10.0).round() / 10.0),
        "ram_mb": st.ram_bytes.map(|b| b >> 20),
        "cpu_percent": st.cpu_percent.map(|c| c.round()),
        "paused_while_empty": st.paused,
        "crashes": st.crashes,
        "last_crash": st.last_crash,
        "release": st.release,
        "update_available": st.update_available,
    })
}

fn playit_json(dir: &Path) -> Value {
    match host::playit(dir) {
        Some(p) if p.online() => json!({ "state": "online", "address": p.address }),
        Some(p) if p.waiting_for_claim() => json!({ "state": "needs claiming", "claim_url": p.claim_url, "do": "the person opens claim_url once to get a public address" }),
        Some(p) => json!({ "state": p.state_text, "notice": p.notice }),
        None => Value::Null,
    }
}

fn port(dir: &Path) -> Option<u16> {
    network::server_port(dir)
}

fn overview() -> Value {
    let servers: Vec<Value> = servers::list()
        .iter()
        .map(|e| {
            let st = servers::status(&e.dir);
            json!({
                "name": e.name,
                "folder": e.dir,
                "remote_host": e.host,
                "phase": st.phase,
                "players": format!("{}/{}", st.players.len(), st.max_players.map_or("?".into(), |m| m.to_string())),
                "tps": st.tps.map(|t| (t * 10.0).round() / 10.0),
                "ram_mb": st.ram_bytes.map(|b| b >> 20),
                "cpu_percent": st.cpu_percent.map(|c| c.round()),
                "port": if e.host.is_none() { port(&e.dir) } else { None },
                "public_address": if e.host.is_none() { playit_json(&e.dir) } else { Value::Null },
            })
        })
        .collect();
    json!({
        "app": format!("HorizonX {}", env!("CARGO_PKG_VERSION")),
        "ready_to_host": host::ready(),
        "horizonsx_engine": host::engine().is_some(),
        "server_release": servers::runtime_release(),
        "servers": servers,
        "network": network_json(false),
    })
}

fn network_json(detail: bool) -> Value {
    let Some(net) = network::load() else { return Value::Null };
    let running = network::running();
    let counts = if running { network::counts() } else { None };
    let mut v = json!({
        "name": net.name,
        "port": net.port,
        "address": if net.address.is_empty() { format!("{}:{}", network::lan_address(), net.port) } else { net.address.clone() },
        "running": running,
        "lobby": net.lobby,
        "servers": net.members.iter().map(|m| json!({ "name": m.name, "port": m.port, "folder": m.dir,
            "players": counts.as_ref().and_then(|(_, c)| c.iter().find(|(n, _)| *n == m.name).map(|(_, x)| *x)) })).collect::<Vec<_>>(),
        "players_total": counts.as_ref().map(|(t, _)| *t),
    });
    if detail {
        v["motd"] = json!(net.motd);
        v["online_mode"] = json!(net.online_mode);
        v["proxy_ram_mb"] = json!(network::memory().map(|b| b >> 20));
    }
    v
}

fn server_status(a: &Value) -> Result<Value, String> {
    let e = find(need(a, "server")?)?;
    let st = servers::status(&e.dir);
    let mut v = json!({ "name": e.name, "folder": e.dir, "status": status_json(&st) });
    if e.host.is_none() {
        let d = &e.dir;
        let sch = host::schedule(d);
        v["port"] = json!(port(d));
        v["public_address"] = playit_json(d);
        v["schedules"] = json!({ "backup_hours": sch.backup_hours, "restart_hour": sch.restart_hour, "restart_every": sch.restart_every });
        v["plugins"] = json!(host::plugin_files(d).iter().map(|(f, on)| json!({ "file": f, "on": on })).collect::<Vec<_>>());
        v["java_layer"] = json!(host::java_layer_on(d));
        v["runs_horizonsx"] = json!(host::runs_horizonsx(d));
    }
    Ok(v)
}

/// The last `n` lines of a file (read from its end).
fn tail(path: &Path, n: usize) -> Vec<String> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else { return vec![] };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    // About 400 bytes a line is plenty; at most the last 4 MB.
    let from = len.saturating_sub(((n as u64 + 1) * 400).clamp(64 << 10, 4 << 20));
    let _ = f.seek(SeekFrom::Start(from));
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    let lines: Vec<&str> = text.lines().collect();
    let skip = usize::from(from > 0);
    lines[skip.min(lines.len())..].iter().rev().take(n).rev().map(|l| l.to_string()).collect()
}

fn log_lines(e: &servers::Entry, n: usize) -> Vec<String> {
    if e.host.is_none() {
        let latest = e.dir.join("logs/latest.log");
        if latest.is_file() {
            return tail(&latest, n);
        }
    }
    hs(e, &["logs", "-n", &n.to_string()]).map(|t| t.lines().map(str::to_string).collect()).unwrap_or_default()
}

fn server_logs(a: &Value) -> Result<Value, String> {
    let e = find(need(a, "server")?)?;
    let n = n(a, "lines").unwrap_or(80).clamp(1, 2000) as usize;
    let lines = match s(a, "contains") {
        Some(c) => {
            let c = c.to_lowercase();
            log_lines(&e, 5000).into_iter().filter(|l| l.to_lowercase().contains(&c)).collect::<Vec<_>>().into_iter().rev().take(n).rev().collect()
        }
        None => log_lines(&e, n),
    };
    Ok(Value::String(if lines.is_empty() { "(nothing logged yet)".into() } else { lines.join("\n") }))
}

fn finding(level: &str, what: String, todo: &str) -> Value {
    json!({ "level": level, "finding": what, "what_to_do": todo })
}

fn diagnose_one(e: &servers::Entry) -> Value {
    let st = servers::status(&e.dir);
    let mut f = vec![];
    if !st.running() {
        f.push(finding("info", "The server is stopped.".into(), "server_start starts it."));
    }
    if st.crashes > 0 {
        f.push(finding("warning", format!("The game crashed {} time(s) since the supervisor started (it restarted by itself). Last: {}", st.crashes, st.last_crash.clone().unwrap_or_default()), "server_logs with contains=\"crash\" or \"exception\" shows why; a plugin named there is the usual cause (plugin_set on=false to rule it out)."));
    }
    if let Some(t) = st.tps.filter(|_| st.running() && !st.paused) {
        if t < 18.0 {
            f.push(finding("warning", format!("TPS is {t:.1} (20 is full speed): the server is lagging."), "server_report records what takes the time; lower view/simulation distance (server_settings_set) or remove a heavy plugin."));
        }
    }
    if let Some(m) = st.mspt.filter(|_| st.running()) {
        if m > 45.0 {
            f.push(finding("warning", format!("Ticks take {m:.0} ms on average (50 ms is the limit)."), "server_report shows which work is slow."));
        }
    }
    if let Some(u) = &st.update_available {
        f.push(finding("info", format!("A newer engine release is out: {u}."), "server_update with check=false installs it (backs up first, rolls back by itself if it fails)."));
    }
    if e.host.is_none() {
        let d = &e.dir;
        match host::playit(d) {
            Some(p) if p.waiting_for_claim() => f.push(finding("action", format!("The public address isn't claimed yet: open {} once.", p.claim_url.clone().unwrap_or_default()), "The person opens the link (free playit.gg account); friends then join from anywhere.")),
            Some(p) if st.running() && !p.online() => f.push(finding("warning", format!("The public address is not online ({}).", p.state_text), "server_command \"playit agent restart\", or check this PC's internet.")),
            None if st.running() && !host::plugin_installed(d, host::PLAYIT) => f.push(finding("info", "No public address plugin (playit): only players on this network can join.".into(), "plugin_add plugin=playit adds it.")),
            _ => {}
        }
        if st.running() && host::engine().is_some() && !host::runs_horizonsx(d) {
            f.push(finding("info", "This server runs the plain Horizon engine, not HorizonsX: plugins need HorizonsX.".into(), "Switch it on the server's Settings page in the app."));
        }
        // The log: errors and plugins that failed to load.
        let lines = log_lines(e, 1500);
        let mut errors: Vec<String> = vec![];
        let mut failed: Vec<String> = vec![];
        for l in &lines {
            let low = l.to_lowercase();
            if low.contains("could not load") || low.contains("error occurred while enabling") || low.contains("load-failed") || low.contains("failed to load plugin") {
                failed.push(l.chars().take(240).collect());
            } else if low.contains("/error]") || low.contains(" error ") || low.contains("exception") || low.contains("[severe]") || low.contains("/severe]") {
                let key: String = l.splitn(2, ']').nth(1).unwrap_or(l).chars().take(160).collect();
                if !errors.iter().any(|x| x.contains(key.trim())) {
                    errors.push(l.chars().take(240).collect());
                }
            }
        }
        if !failed.is_empty() {
            f.push(finding("problem", format!("{} plugin(s) failed to load: {}", failed.len(), failed.iter().take(3).cloned().collect::<Vec<_>>().join(" | ")), "plugins_list shows them; a Modrinth plugin may need a dependency (modrinth_install installs required ones) or not support 26.3."));
        }
        if !errors.is_empty() {
            f.push(finding("warning", format!("{} distinct error(s) in the last {} log lines. First ones: {}", errors.len(), lines.len(), errors.iter().take(4).cloned().collect::<Vec<_>>().join(" | ")), "server_logs with contains=\"error\" or a plugin's name shows the context."));
        }
    }
    if f.iter().all(|x| x["level"] == "info") && st.running() {
        f.insert(0, finding("ok", format!("Running fine{}.", st.tps.map(|t| format!(" at {t:.1} TPS")).unwrap_or_default()), "Nothing needed."));
    }
    json!({ "server": e.name, "status": status_json(&st), "findings": f })
}

fn diagnose(a: &Value) -> Result<Value, String> {
    if let Some(w) = s(a, "server") {
        return Ok(diagnose_one(&find(w)?));
    }
    let all: Vec<Value> = servers::list().iter().map(diagnose_one).collect();
    let mut net = vec![];
    if let Some(n) = network::load() {
        let running = network::running();
        if !running {
            net.push(finding("info", "The network's proxy is stopped: players can't join through it.".into(), "network_start."));
        }
        for m in &n.members {
            if running && !servers::status(&m.dir).running() {
                net.push(finding("warning", format!("{} is in the network but stopped: /server {} can't reach it.", m.name, m.name), "server_start it."));
            }
        }
        if n.members.iter().all(|m| m.name != n.lobby) {
            net.push(finding("problem", format!("The lobby ({}) isn't one of the network's servers.", n.lobby), "network_settings lobby=<a member>."));
        }
    }
    Ok(json!({ "ready_to_host": host::ready(), "servers": all, "network": net }))
}

fn report(a: &Value) -> Result<Value, String> {
    let e = find(need(a, "server")?)?;
    let secs = n(a, "seconds").unwrap_or(30).clamp(5, 300).to_string();
    Ok(Value::String(hs(&e, &["report", &secs, "--no-samples"])?))
}

// ----- Running servers -------------------------------------------------------------------------------------

/// `horizon-server start` stays attached while the server runs: started on its own (not this session's child
/// in any way that ends with it) so the server outlives the MCP session, as one the app started does. What it
/// prints goes to logs/mcp-start.log, to say why when it fails.
fn spawn_start(e: &servers::Entry) -> Result<std::process::Child, String> {
    let mut cmd = e.command(&["start"])?;
    cmd.stdin(Stdio::null());
    if e.host.is_none() {
        let _ = std::fs::create_dir_all(e.dir.join("logs"));
        let out = std::fs::File::create(start_log(&e.dir)).map_err(|x| x.to_string())?;
        cmd.stdout(out.try_clone().map_err(|x| x.to_string())?).stderr(out);
    } else {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB);
        if let Ok(child) = cmd.spawn() {
            return Ok(child);
        }
        // A job that forbids breaking away.
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn().map_err(|e| format!("Couldn't start horizon-server: {e}"))
}

fn start_log(dir: &Path) -> PathBuf {
    dir.join("logs").join("mcp-start.log")
}

fn wait_for(dir: &Path, secs: u64, done: impl Fn(&servers::Status) -> bool) -> servers::Status {
    let until = Instant::now() + Duration::from_secs(secs);
    loop {
        let st = servers::status(dir);
        if done(&st) || Instant::now() > until {
            return st;
        }
        std::thread::sleep(Duration::from_millis(700));
    }
}

fn start(e: &servers::Entry, wait: bool) -> Result<Value, String> {
    if servers::status(&e.dir).running() {
        return Ok(json!({ "server": e.name, "result": "already running" }));
    }
    if e.host.is_none() {
        host::adopt(&e.dir);
    }
    let since = std::fs::metadata(e.dir.join("logs/supervisor.log")).map_or(0, |m| m.len());
    let mut child = spawn_start(e)?;
    if !wait {
        return Ok(json!({ "server": e.name, "result": "starting" }));
    }
    let until = Instant::now() + Duration::from_secs(180);
    let st = loop {
        let st = servers::status(&e.dir);
        if (st.phase == "running" && st.tps.is_some_and(|t| t > 1.0)) || Instant::now() > until {
            break st;
        }
        // `start` ended without a server up: it failed; say why.
        if let Ok(Some(code)) = child.try_wait() {
            if !servers::status(&e.dir).running() {
                let said = std::fs::read_to_string(start_log(&e.dir)).unwrap_or_default();
                let said: String = said.lines().map(|l| l.trim_start_matches(['.', ' '])).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("
");
                let problem = servers::Problem::explain(&said, &e.dir, since);
                return Err(format!("{} didn't start ({code}): {}
{}", e.name, problem.message, log_lines(e, 15).join("
")));
            }
        }
        std::thread::sleep(Duration::from_millis(700));
    };
    if st.running() {
        Ok(json!({ "server": e.name, "result": "running", "status": status_json(&st), "public_address": playit_json(&e.dir) }))
    } else {
        let tailed = log_lines(e, 25).join("\n");
        Err(format!("{} didn't come up within 3 minutes. Its log ends:\n{tailed}", e.name))
    }
}

fn server_action(a: &Value, what: &str) -> Result<Value, String> {
    let e = find(need(a, "server")?)?;
    match what {
        "start" => start(&e, b(a, "wait", true)),
        "stop" => {
            if !servers::status(&e.dir).running() {
                return Ok(json!({ "server": e.name, "result": "already stopped" }));
            }
            hs(&e, &["stop"])?;
            let st = wait_for(&e.dir, 60, |s| !s.running());
            Ok(json!({ "server": e.name, "result": if st.running() { "stopping" } else { "stopped" } }))
        }
        _ => {
            if !servers::status(&e.dir).running() {
                return start(&e, true);
            }
            hs(&e, &["restart"])?;
            std::thread::sleep(Duration::from_secs(3));
            let st = wait_for(&e.dir, 180, |s| s.phase == "running" && s.tps.is_some_and(|t| t > 1.0));
            Ok(json!({ "server": e.name, "result": st.phase, "status": status_json(&st) }))
        }
    }
}

fn create(a: &Value) -> Result<Value, String> {
    if !b(a, "accept_eula", false) {
        return Err("Making a server needs the person's agreement to the Minecraft EULA (https://aka.ms/MinecraftEULA): ask them, then pass accept_eula: true.".into());
    }
    let name = need(a, "name")?.to_string();
    let spec = servers::NewServer {
        name: name.clone(),
        world: match s(a, "copy_world") {
            Some(w) => servers::World::Copy(PathBuf::from(w)),
            None => servers::World::New { seed: s(a, "seed").unwrap_or_default().to_string() },
        },
        game_mode: s(a, "game_mode").unwrap_or("survival").to_string(),
        difficulty: s(a, "difficulty").unwrap_or("normal").to_string(),
        max_players: n(a, "max_players").unwrap_or(10) as u32,
        friends_only: !b(a, "open", false),
        check_accounts: !b(a, "offline", false),
        players: list(a, "players"),
        eula: true,
    };
    let notes = std::sync::Mutex::new(Vec::<String>::new());
    let log = |l: String| notes.lock().unwrap().push(l);
    let tag = host::get_ready(&log)?;
    let entry = servers::create(&spec, Some(&tag), &log).map_err(|p| p.message)?;
    let dir = entry.dir.clone();
    for note in host::adopt(&dir) {
        log(note);
    }
    let mut chosen: Vec<String> = vec![host::PLAYIT.to_string()];
    for p in list(a, "plugins") {
        match catalog_file(&p) {
            Some(f) => chosen.push(f),
            None => log(format!("{p}: not one of HorizonX's plugins (plugin_catalog); modrinth_install adds others")),
        }
    }
    for p in &chosen {
        if !host::plugin_installed(&dir, p) {
            if let Err(e) = host::install_plugin(&dir, p) {
                log(format!("{p}: {e}"));
            }
        }
    }
    let settings = [
        ("properties.horizonsx-region-threads", b(a, "region_threads", true).to_string()),
        ("server.view_distance", n(a, "view_distance").unwrap_or(10).to_string()),
        ("server.simulation_distance", n(a, "simulation_distance").unwrap_or(8).to_string()),
        ("backups.every", if b(a, "backups", true) { "6h".into() } else { "0".into() }),
        ("backups.keep", "3".to_string()),
    ];
    let pairs: Vec<(&str, String)> = settings.iter().map(|(k, v)| (*k, v.clone())).collect();
    if let Err(e) = servers::set_config(&dir, &pairs) {
        log(e.message);
    }
    let mut out = json!({ "created": entry.name, "folder": dir, "port": port(&dir), "plugins": chosen, "notes": notes.lock().unwrap().clone() });
    if b(a, "start", true) {
        out["start"] = start(&entry, true).unwrap_or_else(|e| json!({ "error": e }));
    }
    Ok(out)
}

fn delete(a: &Value) -> Result<Value, String> {
    let (e, d) = local(a)?;
    confirmed(a, "Deleting a server")?;
    if servers::status(&d).running() {
        return Err(format!("{} is running: server_stop it first.", e.name));
    }
    if network::load().is_some_and(|n| n.members.iter().any(|m| m.dir == d)) {
        network::remove(&d)?;
    }
    servers::delete(&d)?;
    Ok(json!({ "deleted": e.name, "note": "its folder is in the trash (recycle bin) if it's needed back" }))
}

fn command(a: &Value) -> Result<Value, String> {
    let e = find(need(a, "server")?)?;
    let line = need(a, "command")?.trim_start_matches('/');
    let out: Vec<String> = if e.host.is_some() {
        hs(&e, &["cmd", line])?.lines().map(str::to_string).collect()
    } else {
        host::command(&e.dir, line).map_err(|p| p.message)?
    };
    Ok(Value::String(if out.is_empty() { "(no reply)".into() } else { out.join("\n") }))
}

fn settings(a: &Value) -> Result<Value, String> {
    let (e, d) = local(a)?;
    servers::config(&d).ok_or_else(|| format!("{}'s server.toml couldn't be read", e.name))
}

fn settings_set(a: &Value) -> Result<Value, String> {
    let (_, d) = local(a)?;
    let changes = a.get("changes").and_then(Value::as_object).ok_or("changes is required: {\"key\": value}")?;
    if changes.is_empty() {
        return Err("changes is empty".into());
    }
    let pairs: Vec<(String, String)> = changes
        .iter()
        .map(|(k, v)| (k.clone(), match v { Value::String(s) => s.clone(), other => other.to_string() }))
        .collect();
    let refs: Vec<(&str, String)> = pairs.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    let done = servers::set_config(&d, &refs).map_err(|p| p.message)?;
    Ok(json!({ "applied_now": done.applied, "at_next_restart": done.restart, "running": done.running }))
}

fn files(a: &Value) -> Result<Value, String> {
    let (_, d) = local(a)?;
    let rel = s(a, "path").unwrap_or("");
    let rows = host::list_files(&d, rel)?;
    Ok(json!(rows.iter().map(|f| json!({ "name": f.name, "folder": f.dir, "bytes": f.bytes, "text": !f.dir && host::editable(&f.name) })).collect::<Vec<_>>()))
}

fn file_read(a: &Value) -> Result<Value, String> {
    let (_, d) = local(a)?;
    Ok(Value::String(host::read_text(&d, need(a, "path")?)?))
}

fn file_write(a: &Value) -> Result<Value, String> {
    let (_, d) = local(a)?;
    let rel = need(a, "path")?;
    let text = a.get("text").and_then(Value::as_str).ok_or("text is required")?;
    let name = Path::new(rel).file_name().and_then(|f| f.to_str()).unwrap_or_default();
    if !host::editable(name) {
        return Err(format!("{name} isn't a text settings file this edits (yml, yaml, json, toml, properties, txt, conf, cfg)"));
    }
    let mut kept = Value::Null;
    if let Ok(old) = host::read_text(&d, rel) {
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let copy = d.join(".horizon").join("edits").join(format!("{stamp}-{}", rel.replace(['/', '\\'], "_")));
        if std::fs::create_dir_all(copy.parent().unwrap()).and_then(|_| std::fs::write(&copy, old)).is_ok() {
            kept = json!(copy.strip_prefix(&d).unwrap_or(&copy));
        }
    }
    host::write_text(&d, rel, text)?;
    Ok(json!({ "written": rel, "bytes": text.len(), "previous_kept_at": kept }))
}

fn schedule(a: &Value) -> Result<Value, String> {
    let (_, d) = local(a)?;
    let mut sch = host::schedule(&d);
    let mut changed = false;
    if let Some(h) = n(a, "backup_hours") {
        sch.backup_hours = h as u32;
        changed = true;
    }
    if let Some(v) = a.get("restart_hour") {
        sch.restart_hour = v.as_u64().map(|h| (h as u32).min(23));
        changed = true;
    }
    if let Some(h) = n(a, "restart_every") {
        sch.restart_every = h as u32;
        changed = true;
    }
    if changed {
        host::save_schedule(&d, &sch)?;
    }
    Ok(json!({ "backup_hours": sch.backup_hours, "restart_hour": sch.restart_hour, "restart_every": sch.restart_every, "saved": changed,
        "note": "The app runs schedules while it is open." }))
}

fn backup(a: &Value) -> Result<Value, String> {
    let e = find(need(a, "server")?)?;
    let mut args = vec!["backup"];
    if let Some(nm) = s(a, "name") {
        args.push(nm);
    }
    Ok(Value::String(hs(&e, &args)?.trim().to_string()))
}

fn backups(a: &Value) -> Result<Value, String> {
    let (_, d) = local(a)?;
    let list = servers::backups(&d).map_err(|p| p.message)?;
    Ok(json!(list.iter().map(|b| json!({ "name": b.name, "mb": (b.bytes as f64 / 1048576.0 * 10.0).round() / 10.0, "modified_unix": b.modified })).collect::<Vec<_>>()))
}

fn restore(a: &Value) -> Result<Value, String> {
    let e = find(need(a, "server")?)?;
    confirmed(a, "Restoring a backup replaces the world")?;
    let backup = need(a, "backup")?;
    let mut args = vec!["restore", backup];
    if let Some(dim) = s(a, "dimension") {
        args.extend(["--dimension", dim]);
    }
    args.push("--yes");
    Ok(Value::String(hs(&e, &args)?.trim().to_string()))
}

fn update(a: &Value) -> Result<Value, String> {
    let e = find(need(a, "server")?)?;
    if b(a, "check", true) {
        return hs_json(&e, &["update", "--check", "--json"]);
    }
    Ok(Value::String(hs(&e, &["update"])?.trim().to_string()))
}

fn panel(a: &Value) -> Result<Value, String> {
    let (_, d) = local(a)?;
    let api = servers::Api::open(&d).ok_or("The server isn't running")?;
    Ok(Value::String(api.panel_link()?))
}

fn players(a: &Value) -> Result<Value, String> {
    let e = find(need(a, "server")?)?;
    hs_json(&e, &["players", "--json"])
}

fn player_action(a: &Value) -> Result<Value, String> {
    let e = find(need(a, "server")?)?;
    let action = need(a, "action")?;
    let player = s(a, "player");
    let reason = s(a, "reason");
    let who = || player.ok_or_else(|| format!("{action} needs player"));
    let mut args: Vec<&str> = match action {
        "op" | "deop" | "kick" | "ban" | "pardon" => vec![action, who()?],
        "ban_ip" => vec!["ban-ip", who()?],
        "pardon_ip" => vec!["pardon-ip", who()?],
        "whitelist_add" => vec!["whitelist", "add", who()?],
        "whitelist_remove" => vec!["whitelist", "remove", who()?],
        "whitelist_on" => vec!["whitelist", "on"],
        "whitelist_off" => vec!["whitelist", "off"],
        other => return Err(format!("Unknown action {other}")),
    };
    if let (Some(r), true) = (reason, matches!(action, "kick" | "ban" | "ban_ip")) {
        args.push(r);
    }
    let out = hs(&e, &args)?;
    Ok(Value::String(if out.trim().is_empty() { "Done.".into() } else { out.trim().to_string() }))
}

// ----- Plugins ------------------------------------------------------------------------------------------------

/// A shipped plugin's file from its file or name (any case; "playit" finds playit_rs.dll).
fn catalog_file(which: &str) -> Option<String> {
    let w = which.to_lowercase();
    let cat = host::catalog();
    cat.iter()
        .find(|p| p.file.to_lowercase() == w || p.name.to_lowercase() == w)
        .or_else(|| cat.iter().find(|p| p.file.to_lowercase().trim_start_matches("lib").starts_with(&w) || p.name.to_lowercase().contains(&w)))
        .map(|p| p.file.clone())
}

fn catalog() -> Value {
    json!(host::catalog().iter().map(|p| json!({ "file": p.file, "name": p.name, "description": p.description, "always_added": p.built_in })).collect::<Vec<_>>())
}

fn plugins_list(a: &Value) -> Result<Value, String> {
    let (_, d) = local(a)?;
    let cat = host::catalog();
    let from_modrinth = plugin_market::installed(&d);
    let java = host::java_plugins(&d);
    let files: Vec<Value> = host::plugin_files(&d)
        .iter()
        .map(|(f, on)| {
            let mut v = json!({ "file": f, "on": on });
            if let Some(c) = cat.iter().find(|c| &c.file == f) {
                v["name"] = json!(c.name);
                v["kind"] = json!("HorizonX plugin");
            }
            if let Some(j) = java.iter().find(|j| &j.file == f) {
                v["name"] = json!(j.name);
                v["kind"] = json!("Java plugin");
                v["version"] = json!(j.version);
                v["authors"] = json!(j.authors);
                v["configs"] = json!(j.configs);
            }
            if let Some((_, r)) = from_modrinth.iter().find(|(_, r)| &r.file == f) {
                v["modrinth"] = json!({ "name": r.name, "version": r.version });
            }
            v
        })
        .collect();
    Ok(json!({ "plugins": files, "java_layer": host::java_layer_on(&d) }))
}

fn plugin_add(a: &Value) -> Result<Value, String> {
    let p = need(a, "plugin")?;
    let file = catalog_file(p).ok_or_else(|| format!("{p} isn't one of HorizonX's plugins (plugin_catalog); modrinth_install adds others"))?;
    let mut done = Map::new();
    for e in many(a)? {
        let r = if e.host.is_some() {
            "on another computer: not reachable".to_string()
        } else if host::plugin_installed(&e.dir, &file) {
            "already there".into()
        } else {
            host::install_plugin(&e.dir, &file).map(|_| "added".to_string()).unwrap_or_else(|x| x)
        };
        done.insert(e.name, json!(r));
    }
    Ok(json!({ "plugin": file, "servers": done }))
}

fn plugin_file_in(d: &Path, which: &str) -> Result<String, String> {
    let files = host::plugin_files(d);
    let w = which.to_lowercase();
    files
        .iter()
        .find(|(f, _)| f.to_lowercase() == w)
        .or_else(|| files.iter().find(|(f, _)| f.to_lowercase().starts_with(&w)))
        .or_else(|| {
            let java = host::java_plugins(d);
            java.iter().find(|j| j.name.to_lowercase() == w).and_then(|j| files.iter().find(|(f, _)| *f == j.file))
        })
        .map(|(f, _)| f.clone())
        .ok_or_else(|| format!("No plugin {which} in this server (plugins_list)"))
}

fn plugin_remove(a: &Value) -> Result<Value, String> {
    let (_, d) = local(a)?;
    confirmed(a, "Removing a plugin")?;
    let f = plugin_file_in(&d, need(a, "plugin")?)?;
    host::remove_plugin(&d, &f)?;
    let _ = std::fs::remove_file(d.join("plugins").join(format!("{f}.disabled")));
    Ok(json!({ "removed": f, "note": "its data folder stays" }))
}

fn plugin_set(a: &Value) -> Result<Value, String> {
    let (_, d) = local(a)?;
    let on = a.get("on").and_then(Value::as_bool).ok_or("on is required")?;
    let asked = need(a, "plugin")?;
    // The Java layer: remembered per server, so adding a jar later doesn't put it back.
    if matches!(asked.to_ascii_lowercase().replace(['-', '_'], " ").as_str(), "java layer" | "java" | "horizon java") || asked.contains("horizon_java") {
        host::set_java_layer(&d, on)?;
        return Ok(json!({ "java_layer": on, "note": "the server picks it up at its next restart" }));
    }
    let f = plugin_file_in(&d, asked)?;
    if f == host::PLAYIT {
        // playit (the public address): remembered per server, so opening it again doesn't put it back.
        host::set_playit(&d, on)?;
        return Ok(json!({ "plugin": f, "on": on, "note": if on { "the public address comes back (a running server loads it at once)" } else { "no public address: only players on this network can join" } }));
    }
    host::set_plugin_on(&d, &f, on)?;
    Ok(json!({ "plugin": f, "on": on, "note": "takes effect at the next restart" }))
}

fn add_jar(a: &Value) -> Result<Value, String> {
    let (_, d) = local(a)?;
    let jars: Vec<PathBuf> = list(a, "paths").into_iter().map(PathBuf::from).collect();
    if let Some(missing) = jars.iter().find(|j| !j.is_file()) {
        return Err(format!("{} isn't a file", missing.display()));
    }
    let n = host::add_jars(&d, &jars)?;
    Ok(json!({ "added": n, "note": "restart the server to load them" }))
}

fn modrinth_search(a: &Value) -> Result<Value, String> {
    let r = plugin_market::search(s(a, "query").unwrap_or(""), n(a, "offset").unwrap_or(0) as usize)?;
    Ok(json!({ "total": r.total, "plugins": r.hits.iter().map(|h| json!({ "id": h.id, "slug": h.slug, "title": h.title, "author": h.author, "description": h.description, "downloads": h.downloads })).collect::<Vec<_>>() }))
}

fn modrinth_install(a: &Value) -> Result<Value, String> {
    let project = need(a, "project")?;
    let mut done = Map::new();
    for e in many(a)? {
        if e.host.is_some() {
            done.insert(e.name, json!("on another computer: not reachable"));
            continue;
        }
        let r = match plugin_market::install(&e.dir, project) {
            Ok(i) => {
                let _ = host::ensure_java_layer(&e.dir);
                json!({ "installed": i.name, "file": i.file, "dependencies": i.with, "already_there": i.already, "restart_needed": servers::status(&e.dir).running() })
            }
            Err(x) => json!({ "error": x }),
        };
        done.insert(e.name, r);
    }
    Ok(json!({ "project": project, "servers": done }))
}

// ----- Network ------------------------------------------------------------------------------------------------

fn network_create(a: &Value) -> Result<Value, String> {
    if network::load().is_some() {
        return Err("A network exists already: network_add_server adds servers to it, network_settings changes it.".into());
    }
    let chosen: Vec<(String, PathBuf)> = list(a, "servers").iter().map(|w| find(w).map(|e| (e.name, e.dir))).collect::<Result<_, _>>()?;
    if chosen.is_empty() {
        return Err("servers is required".into());
    }
    let lobby = match s(a, "lobby") {
        Some(l) => chosen.iter().position(|(nm, d)| nm.eq_ignore_ascii_case(l) || d.file_name().is_some_and(|f| f.eq_ignore_ascii_case(l))).ok_or_else(|| format!("{l} isn't one of the servers given"))?,
        None => chosen.iter().position(|(nm, _)| ["lobby", "hub"].contains(&nm.to_lowercase().as_str())).unwrap_or(0),
    };
    let name = s(a, "name").unwrap_or("My network");
    let port = n(a, "port").unwrap_or(25565) as u16;
    let net = network::create(name, port, &chosen, lobby)?;
    let mut out = json!({ "created": net.name, "port": net.port, "lobby": net.lobby, "servers": net.members.iter().map(|m| &m.name).collect::<Vec<_>>(),
        "note": "restart the member servers that run so they take the network settings" });
    if b(a, "start", true) {
        out["proxy"] = match network::start() {
            Ok(pid) => json!({ "running": true, "pid": pid }),
            Err(e) => json!({ "error": e }),
        };
    }
    Ok(out)
}

fn network_settings(a: &Value) -> Result<Value, String> {
    let mut net = network::load().ok_or("No network yet: network_create makes one")?;
    if let Some(v) = s(a, "name") {
        net.name = v.to_string();
    }
    if let Some(v) = n(a, "port") {
        net.port = v as u16;
    }
    if let Some(v) = a.get("motd").and_then(Value::as_str) {
        net.motd = v.to_string();
    }
    if let Some(v) = a.get("online_mode").and_then(Value::as_bool) {
        net.online_mode = v;
    }
    if let Some(v) = a.get("address").and_then(Value::as_str) {
        net.address = v.trim().to_string();
    }
    if let Some(l) = s(a, "lobby") {
        let m = net.members.iter().find(|m| m.name.eq_ignore_ascii_case(l) || m.dir.file_name().is_some_and(|f| f.eq_ignore_ascii_case(l))).ok_or_else(|| format!("{l} isn't in the network"))?;
        net.lobby = m.name.clone();
    }
    network::update(&net)?;
    let restarted = if network::running() { network::restart().map(|_| true)? } else { false };
    Ok(json!({ "saved": true, "proxy_restarted": restarted, "network": network_json(true) }))
}

fn network_member(a: &Value, add: bool) -> Result<Value, String> {
    let e = find(need(a, "server")?)?;
    let net = if add { network::add(&e.name, &e.dir)? } else { network::remove(&e.dir)? };
    Ok(json!({ "network": net.name, "servers": net.members.iter().map(|m| &m.name).collect::<Vec<_>>(),
        "note": if add { "restart the server (if running) and the proxy to route players to it" } else { "restart the proxy to drop it" } }))
}

fn network_power(what: &str) -> Result<Value, String> {
    network::load().ok_or("No network yet: network_create makes one")?;
    match what {
        "start" => network::start().map(|pid| json!({ "running": true, "pid": pid })),
        "stop" => network::stop().map(|_| json!({ "running": false })),
        _ => network::restart().map(|pid| json!({ "running": true, "pid": pid })),
    }
}

fn network_delete(a: &Value) -> Result<Value, String> {
    confirmed(a, "Removing the network")?;
    if network::running() {
        network::stop()?;
    }
    network::delete()?;
    Ok(json!({ "deleted": true }))
}

// ----- Dispatch -------------------------------------------------------------------------------------------------

pub fn call(name: &str, a: &Value) -> Result<Value, String> {
    match name {
        "horizonx_overview" => Ok(overview()),
        "server_status" => server_status(a),
        "diagnose" => diagnose(a),
        "server_logs" => server_logs(a),
        "server_report" => report(a),
        "server_create" => create(a),
        "server_start" => server_action(a, "start"),
        "server_stop" => server_action(a, "stop"),
        "server_restart" => server_action(a, "restart"),
        "server_delete" => delete(a),
        "server_command" => command(a),
        "server_settings" => settings(a),
        "server_settings_set" => settings_set(a),
        "server_files" => files(a),
        "server_file_read" => file_read(a),
        "server_file_write" => file_write(a),
        "server_schedule" => schedule(a),
        "server_backup" => backup(a),
        "server_backups" => backups(a),
        "server_restore" => restore(a),
        "server_update" => update(a),
        "server_panel_link" => panel(a),
        "players" => players(a),
        "player_action" => player_action(a),
        "plugin_catalog" => Ok(catalog()),
        "plugins_list" => plugins_list(a),
        "plugin_add" => plugin_add(a),
        "plugin_remove" => plugin_remove(a),
        "plugin_set" => plugin_set(a),
        "plugin_add_jar" => add_jar(a),
        "modrinth_search" => modrinth_search(a),
        "modrinth_install" => modrinth_install(a),
        "network_status" => Ok(network_json(true)),
        "network_create" => network_create(a),
        "network_settings" => network_settings(a),
        "network_add_server" => network_member(a, true),
        "network_remove_server" => network_member(a, false),
        "network_start" => network_power("start"),
        "network_stop" => network_power("stop"),
        "network_restart" => network_power("restart"),
        "network_log" => Ok(Value::String(network::log_tail(n(a, "lines").unwrap_or(80).clamp(1, 2000) as usize).join("\n"))),
        "network_delete" => network_delete(a),
        "horizonx_setup" => {
            let notes = std::sync::Mutex::new(Vec::<String>::new());
            let tag = host::get_ready(&|l| notes.lock().unwrap().push(l))?;
            Ok(json!({ "ready": host::ready(), "release": tag, "notes": notes.into_inner().unwrap() }))
        }
        other => Err(format!("Unknown tool {other}")),
    }
}

// ----- Connecting assistants ----------------------------------------------------------------------------------

/// The command an MCP client runs: this program with `mcp`.
pub fn launch_command() -> (String, Vec<String>) {
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "HorizonX".into());
    (exe, vec!["mcp".into()])
}

/// The JSON block MCP clients (Claude Desktop, Cursor, VS Code...) take under `mcpServers`.
pub fn client_config() -> String {
    let (cmd, args) = launch_command();
    serde_json::to_string_pretty(&json!({ "mcpServers": { "horizonx": { "command": cmd, "args": args } } })).unwrap_or_default()
}

/// Adds HorizonX to a client's config file (`mcpServers.horizonx`), keeping everything else in it.
fn add_to_file(path: &Path) -> Result<(), String> {
    let mut root: Value = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| json!({}));
    if !root.is_object() {
        return Err(format!("{} isn't a JSON object", path.display()));
    }
    let (cmd, args) = launch_command();
    let servers = root.as_object_mut().unwrap().entry("mcpServers").or_insert_with(|| json!({}));
    servers.as_object_mut().ok_or("mcpServers isn't an object")?.insert("horizonx".into(), json!({ "command": cmd, "args": args }));
    std::fs::create_dir_all(path.parent().ok_or("no folder")?).map_err(|e| e.to_string())?;
    if path.is_file() {
        let _ = std::fs::copy(path, path.with_extension("json.bak"));
    }
    std::fs::write(path, serde_json::to_string_pretty(&root).unwrap_or_default()).map_err(|e| e.to_string())
}

/// An assistant HorizonX can connect itself to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Client {
    ClaudeCode,
    ClaudeDesktop,
    Cursor,
}

impl Client {
    pub const ALL: [Client; 3] = [Client::ClaudeCode, Client::ClaudeDesktop, Client::Cursor];

    pub fn label(self) -> &'static str {
        match self {
            Client::ClaudeCode => "Claude Code",
            Client::ClaudeDesktop => "Claude Desktop",
            Client::Cursor => "Cursor",
        }
    }

    fn file(self) -> Option<PathBuf> {
        match self {
            Client::ClaudeCode => None,
            Client::ClaudeDesktop => {
                if cfg!(windows) {
                    dirs_config().map(|d| d.join("Claude").join("claude_desktop_config.json"))
                } else if cfg!(target_os = "macos") {
                    home().map(|h| h.join("Library/Application Support/Claude/claude_desktop_config.json"))
                } else {
                    home().map(|h| h.join(".config/Claude/claude_desktop_config.json"))
                }
            }
            Client::Cursor => home().map(|h| h.join(".cursor").join("mcp.json")),
        }
    }

    /// Whether HorizonX is in its config already.
    pub fn connected(self) -> bool {
        match self {
            Client::ClaudeCode => home()
                .map(|h| h.join(".claude.json"))
                .and_then(|p| std::fs::read_to_string(p).ok())
                .is_some_and(|t| t.contains("\"horizonx\"")),
            _ => self
                .file()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                .is_some_and(|v| v.pointer("/mcpServers/horizonx").is_some()),
        }
    }

    /// Connects HorizonX: `claude mcp add` for Claude Code, the config file for the others.
    pub fn connect(self) -> Result<String, String> {
        match self {
            Client::ClaudeCode => {
                let (cmd, args) = launch_command();
                let claude = if cfg!(windows) { "claude.cmd" } else { "claude" };
                let mut c = horizon_launcher::process::command(claude);
                c.args(["mcp", "add", "--scope", "user", "horizonx", "--", &cmd]).args(&args).stdin(Stdio::null());
                let out = c.output().or_else(|_| {
                    let mut c = horizon_launcher::process::command("claude");
                    c.args(["mcp", "add", "--scope", "user", "horizonx", "--", &cmd]).args(&args).stdin(Stdio::null());
                    c.output()
                });
                match out {
                    Ok(o) if o.status.success() => Ok("Added to Claude Code: start a new Claude Code session to use it.".into()),
                    Ok(o) => {
                        let text = String::from_utf8_lossy(&o.stderr).to_string() + &String::from_utf8_lossy(&o.stdout);
                        if text.contains("already exists") { Ok("Claude Code has HorizonX already.".into()) } else { Err(text.trim().to_string()) }
                    }
                    Err(_) => Err("Claude Code isn't installed here (the `claude` command wasn't found).".into()),
                }
            }
            _ => {
                let path = self.file().ok_or("No home folder")?;
                add_to_file(&path)?;
                Ok(format!("Added to {}: restart it to use HorizonX.", self.label()))
            }
        }
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

fn dirs_config() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connecting_keeps_the_clients_other_servers() {
        let dir = std::env::temp_dir().join(format!("hx-mcp-test-{}", std::process::id()));
        let path = dir.join("mcp.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, r#"{"theme":"dark","mcpServers":{"other":{"command":"x"}}}"#).unwrap();
        add_to_file(&path).unwrap();
        add_to_file(&path).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        assert_eq!(v["mcpServers"]["horizonx"]["args"][0], "mcp");
        assert!(path.with_extension("json.bak").is_file());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn every_tool_is_dispatched_and_described() {
        // Checked against the source, not by calling them: a call would act on this PC's real servers.
        let src = include_str!("mcp.rs");
        let dispatch = &src[src.find("pub fn call(").unwrap()..];
        for t in tools() {
            let name = t["name"].as_str().unwrap();
            assert!(t["description"].as_str().unwrap().len() > 20, "{name}");
            assert!(dispatch.contains(&format!("\"{name}\" =>")), "{name} has no handler");
        }
    }
}
