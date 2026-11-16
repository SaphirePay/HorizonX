//! MiniMOTD 2.2.5 for Horizon, written again in Rust (the Java it follows is in java/). Same files
//! (`plugins/MiniMOTD/main.conf`, `plugin_settings.conf`, `extra-configs/*.conf`, `icons/*.png`), same `/minimotd`.
//!
//! A server-list ping runs on the connection's own thread: it reads the settings parsed at start/reload (one
//! read lock), picks a MOTD, fills `<online_players>`/`<max_players>`, and parses the MiniMessage there. No game
//! query, nothing on the server thread.

use horizon_bukkit::adventure::Component;
use horizon_bukkit::fam_core::CachedServerIcon;
use horizon_bukkit::fam_ev_world::PaperServerListPingEvent;
use horizon_bukkit::{CommandSender, Plugin, bukkit_plugin};
use horizon_kit::{hocon, mini};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};

static PLUGIN: Mutex<Option<Plugin>> = Mutex::new(None);
static STATE: RwLock<Option<State>> = RwLock::new(None);

#[derive(Clone, Debug)]
enum Modifier {
    Add(i64),
    Constant(i64),
    Minimum(i64),
    Random(i64, i64),
    Percent(f64),
}

impl Modifier {
    fn parse(input: &str) -> Option<Self> {
        let s = input.trim();
        Some(if let Some((a, b)) = s.split_once(':') {
            Self::Random(a.trim().parse().ok()?, b.trim().parse().ok()?)
        } else if s.contains('%') {
            Self::Percent(s.replace('%', "").trim().parse().ok()?)
        } else if s.contains('=') {
            Self::Constant(s.replace('=', "").trim().parse().ok()?)
        } else if s.contains('+') {
            Self::Minimum(s.replace('+', "").trim().parse().ok()?)
        } else {
            Self::Add(s.parse().ok()?)
        })
    }
    fn apply(&self, actual: i64) -> i64 {
        match *self {
            Self::Add(n) => actual + n,
            Self::Constant(n) => n,
            Self::Minimum(n) => n.max(actual),
            Self::Random(a, b) => actual + if b > a { a + (rand() % (b - a) as u64) as i64 } else { a },
            Self::Percent(p) => ((1.0 + p / 100.0) * actual as f64).ceil() as i64,
        }
    }
}

/// A cheap random number (xorshift on a shared seed): which MOTD and icon to show.
fn rand() -> u64 {
    static SEED: AtomicU64 = AtomicU64::new(0);
    let mut x = SEED.load(Ordering::Relaxed);
    if x == 0 {
        x = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(88172645463325252, |d| d.as_nanos() as u64) | 1;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    SEED.store(x, Ordering::Relaxed);
    x
}

#[derive(Clone, Debug)]
struct Motd {
    line1: String,
    line2: String,
    icon: String,
}

/// One `.conf` of MOTDs (main.conf or an extra config).
#[derive(Clone, Debug)]
struct MotdConfig {
    motds: Vec<Motd>,
    motd_enabled: bool,
    icon_enabled: bool,
    max_players_enabled: bool,
    max_players: i64,
    disable_hover: bool,
    hide_count: bool,
    fake_enabled: bool,
    fake: Modifier,
    x_more_enabled: bool,
    x_value: i64,
    allow_exceeding: bool,
}

impl MotdConfig {
    fn read(v: &Value) -> Self {
        let pcs = &v["player-count-settings"];
        let b = |v: &Value, d: bool| v.as_bool().unwrap_or(d);
        let i = |v: &Value, d: i64| v.as_i64().unwrap_or(d);
        let s = |v: &Value, d: &str| v.as_str().map_or_else(|| d.to_owned(), str::to_owned);
        let motds = v["motds"]
            .as_array()
            .map(|a| a.iter().map(|m| Motd { line1: s(&m["line1"], ""), line2: s(&m["line2"], ""), icon: s(&m["icon"], "random") }).collect())
            .unwrap_or_default();
        let fake = &pcs["fake-players"]["fake-players"];
        let fake_text = fake.as_str().map(str::to_owned).or_else(|| fake.as_i64().map(|n| n.to_string()));
        Self {
            motds,
            motd_enabled: b(&v["motd-enabled"], true),
            icon_enabled: b(&v["icon-enabled"], true),
            max_players_enabled: b(&pcs["max-players-enabled"], true),
            max_players: i(&pcs["max-players"], 69),
            disable_hover: b(&pcs["disable-player-list-hover"], false),
            hide_count: b(&pcs["hide-player-count"], false),
            fake_enabled: b(&pcs["fake-players"]["fake-players-enabled"], false),
            fake: fake_text.as_deref().and_then(Modifier::parse).unwrap_or(Modifier::Percent(25.0)),
            x_more_enabled: b(&pcs["just-x-more-settings"]["just-x-more-enabled"], false),
            x_value: i(&pcs["just-x-more-settings"]["x-value"], 3),
            allow_exceeding: b(&pcs["allow-exceeding-maximum"], false),
        }
    }
    /// `modifyPlayerCount`.
    fn counts(&self, online: i64, max: i64) -> (i64, i64) {
        let online = if self.fake_enabled { self.fake.apply(online) } else { online };
        let max = if self.max_players_enabled { if self.x_more_enabled { online + self.x_value } else { self.max_players } } else { max };
        if self.allow_exceeding { (online, max) } else { (online.min(max), max) }
    }
}

struct State {
    main: MotdConfig,
    icons: Vec<(String, CachedServerIcon)>,
}

const MAIN_DEFAULT: &str = r#"# MiniMOTD Main Configuration

# Enable server list icon related features
icon-enabled=true
# Enable MOTD-related features
motd-enabled=true
# The list of MOTDs to display
#
#  - Supported placeholders: <online_players>, <max_players>
#  - Putting more than one will cause one to be randomly chosen each refresh
motds=[
    {
        # Set the icon to use with this MOTD
        #   Either use 'random' to randomly choose an icon, or use the name
        #   of a file in the icons folder (excluding the '.png' extension)
        #     ex: icon="myIconFile"
        icon=random
        line1="<rainbow>MiniMOTD Default"
        line2="MiniMessage <gradient:blue:red>Gradients"
    },
    {
        icon=random
        line1="<blue>Another <bold><red>MOTD"
        line2="<italic><underlined><gradient:red:green>much wow"
    }
]
player-count-settings {
    # Should the displayed online player count be allowed to exceed the displayed maximum player count?
    # If false, the online player count will be capped at the maximum player count
    allow-exceeding-maximum=false
    # Setting this to true will disable the hover text showing online player usernames
    disable-player-list-hover=false
    # Settings for the fake player count feature
    fake-players {
        # Modes: add, constant, minimum, random, percent
        #
        #  - add: This many fake players will be added
        #      ex: fake-players="3"
        #  - constant: A constant value for the player count
        #      ex: fake-players="=42"
        #  - minimum: The minimum bound of the player count
        #      ex: fake-players="7+"
        #  - random: A random number of fake players in this range will be added
        #      ex: fake-players="3:6"
        #  - percent: The player count will be inflated by this much, rounding up
        #      ex: fake-players="25%"
        fake-players="25%"
        # Enable fake player count feature
        fake-players-enabled=false
    }
    # Setting this to true will disable the player list hover (same as 'disable-player-list-hover'),
    # but will also cause the player count to appear as '???'
    hide-player-count=false
    # Changes the Max Players to be X more than the online players
    # ex: x=3 -> 16/19 players online.
    just-x-more-settings {
        # Enable this feature
        just-x-more-enabled=false
        x-value=3
    }
    # Changes the Max Players value
    max-players=69
    # Enable modification of the max player count
    max-players-enabled=true
    # The list of server names that affect player counts/listing.
    # Only applicable when running the plugin on a proxy (Velocity or Waterfall/Bungeecord).
    # When set to an empty list, the default count & list as determined by the proxy will be used.
    servers=[]
}
"#;

const SETTINGS_DEFAULT: &str = r#"# MiniMOTD Plugin Configuration

# Settings only applicable when running the plugin on a proxy (Velocity or Waterfall/Bungeecord)
proxy-settings {
    virtual-host-configs {
        "minigames.example.com:25565"=default
        "skyblock.example.com:25565"=skyblock
        "survival.example.com:25565"=survival
    }
    virtual-host-test-mode=false
}
# Do you want the plugin to check for updates on GitHub at launch?
# https://github.com/jpenilla/MiniMOTD
update-checker=false
"#;

fn extra_default(line1: &str, line2: &str) -> String {
    MAIN_DEFAULT
        .replacen("<rainbow>MiniMOTD Default", line1, 1)
        .replacen("MiniMessage <gradient:blue:red>Gradients", line2, 1)
        .replace(
            "    },\n    {\n        icon=random\n        line1=\"<blue>Another <bold><red>MOTD\"\n        line2=\"<italic><underlined><gradient:red:green>much wow\"\n    }\n",
            "    }\n",
        )
        .replacen("# MiniMOTD Main Configuration", "# Extra MiniMOTD config", 1)
}

/// A 64x64 PNG, read from its header.
fn png_64(bytes: &[u8]) -> bool {
    bytes.len() > 24 && bytes.starts_with(&[0x89, b'P', b'N', b'G']) && {
        let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        w == 64 && h == 64
    }
}

fn load() -> Result<State, String> {
    let plugin = (*PLUGIN.lock().unwrap_or_else(|e| e.into_inner())).ok_or("not enabled")?;
    let dir = plugin.get_data_folder();
    let _ = std::fs::create_dir_all(&dir);
    let main_path = dir.join("main.conf");
    if !main_path.exists() {
        let _ = std::fs::write(&main_path, MAIN_DEFAULT);
    }
    let settings_path = dir.join("plugin_settings.conf");
    if !settings_path.exists() {
        let _ = std::fs::write(&settings_path, SETTINGS_DEFAULT);
    }
    let main = MotdConfig::read(&hocon::read(&main_path)?);
    hocon::read(&settings_path)?;

    // Extra configs are a proxy feature (virtual hosts); on a server MiniMOTD only checks they read.
    let extra_dir = dir.join("extra-configs");
    if !extra_dir.exists() {
        let _ = std::fs::create_dir_all(&extra_dir);
        let _ = std::fs::write(extra_dir.join("skyblock.conf"), extra_default("<green><italic>Skyblock</green>", "<bold><rainbow>MiniMOTD Skyblock Server"));
        let _ = std::fs::write(extra_dir.join("survival.conf"), extra_default("<gradient:blue:red>Survival Mode Hardcore", "<green><bold>MiniMOTD Survival Server"));
    }
    for entry in std::fs::read_dir(&extra_dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "conf") {
            hocon::read(&path)?;
        }
    }

    let icon_dir = dir.join("icons");
    let _ = std::fs::create_dir_all(&icon_dir);
    let mut icons = Vec::new();
    for entry in std::fs::read_dir(&icon_dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if !path.is_file() || !name.ends_with(".png") {
            continue;
        }
        match std::fs::read(&path) {
            Ok(bytes) if png_64(&bytes) => {
                if let Some(icon) = horizon_bukkit::fam_core::load_server_icon(&path) {
                    icons.push((name.split('.').next().unwrap_or_default().to_owned(), icon));
                }
            }
            Ok(_) => plugin.warning(&format!("Could not load {name}: image must be 64x64px")),
            Err(_) => plugin.warning(&format!("Could not load {name}: invalid image file")),
        }
    }
    if main.motd_enabled && main.motds.is_empty() {
        return Err("MOTD is enabled, but there are no MOTDs in the config file?".into());
    }
    Ok(State { main, icons })
}

fn reload() -> Result<(), String> {
    let state = load()?;
    *STATE.write().unwrap_or_else(|e| e.into_inner()) = Some(state);
    Ok(())
}

/// Hex colours down to the nearest of the 16 for clients older than 1.16 (`ComponentColorDownsampler`).
fn downsample(v: &mut Value) {
    const NAMED: [(&str, u32); 16] = [
        ("black", 0x000000), ("dark_blue", 0x0000AA), ("dark_green", 0x00AA00), ("dark_aqua", 0x00AAAA), ("dark_red", 0xAA0000), ("dark_purple", 0xAA00AA),
        ("gold", 0xFFAA00), ("gray", 0xAAAAAA), ("dark_gray", 0x555555), ("blue", 0x5555FF), ("green", 0x55FF55), ("aqua", 0x55FFFF), ("red", 0xFF5555),
        ("light_purple", 0xFF55FF), ("yellow", 0xFFFF55), ("white", 0xFFFFFF),
    ];
    if let Some(hex) = v["color"].as_str().and_then(|c| c.strip_prefix('#')).and_then(|h| u32::from_str_radix(h, 16).ok()) {
        let d = |a: u32, b: u32| {
            let c = |s: u32| (((a >> s) & 0xFF) as i64 - ((b >> s) & 0xFF) as i64).pow(2);
            c(16) + c(8) + c(0)
        };
        if let Some((name, _)) = NAMED.iter().min_by_key(|(_, rgb)| d(hex, *rgb)) {
            v["color"] = json!(name);
        }
    }
    if let Some(extra) = v.get_mut("extra").and_then(Value::as_array_mut) {
        extra.iter_mut().for_each(downsample);
    }
}

fn line(text: &str, online: &str, max: &str) -> Value {
    let text = text.replace("{onlinePlayers}", online).replace("{maxPlayers}", max);
    mini::parse_with(&text, mini::Opts::default(), &|name, _| match name {
        "online_players" => Some(json!({ "text": online })),
        "max_players" => Some(json!({ "text": max })),
        _ => None,
    })
}

fn on_ping(event: &mut PaperServerListPingEvent) {
    let held = STATE.read().unwrap_or_else(|e| e.into_inner());
    let Some(state) = held.as_ref() else { return };
    let cfg = &state.main;
    let (online, max) = cfg.counts(event.get_num_players().max(0), event.get_max_players());
    event.set_num_players(online);
    event.set_max_players(max);
    let mut icon_name: Option<&str> = None;
    if cfg.motd_enabled && !cfg.motds.is_empty() {
        let m = &cfg.motds[if cfg.motds.len() == 1 { 0 } else { (rand() % cfg.motds.len() as u64) as usize }];
        let (o, x) = (online.to_string(), max.to_string());
        let mut motd = json!({ "text": "", "extra": [line(&m.line1, &o, &x), "\n", line(&m.line2, &o, &x)] });
        if event.get_protocol_version() < 735 {
            downsample(&mut motd);
        }
        event.set_motd_component(&Component(motd));
        icon_name = Some(&m.icon);
    }
    if cfg.icon_enabled && !state.icons.is_empty() {
        let icon = match icon_name {
            Some(name) if name != "random" => state.icons.iter().find(|(n, _)| n == name).map(|(_, i)| i),
            _ => state.icons.get((rand() % state.icons.len() as u64) as usize).map(|(_, i)| i),
        };
        if let Some(icon) = icon {
            event.set_server_icon(Some(icon));
        }
    }
    if cfg.disable_hover {
        event.get_listed_players().clear();
    }
    if cfg.hide_count {
        event.set_hide_players(true);
    }
}

const PREFIX: &str = "<white>[</white><gradient:#0047AB:#ADD8E6>MiniMOTD</gradient><white>]</white>";

fn send(sender: &CommandSender, mini_text: &str) {
    sender.send_component(&Component(mini::parse(mini_text)));
}

fn command(sender: &CommandSender, args: &[&str]) -> bool {
    if !sender.has_permission("minimotd.admin") {
        sender.send_message("§cI'm sorry, but you do not have permission to perform this command.");
        return true;
    }
    match args.first().map(|a| a.to_ascii_lowercase()).as_deref() {
        Some("reload") => match reload() {
            Ok(()) => send(sender, &format!("{PREFIX} <green>Done reloading configuration.")),
            Err(e) => {
                if let Some(p) = *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) {
                    p.warning(&format!("Failed to reload MiniMOTD. Ensure there are no errors in your config files. {e}"));
                }
                send(sender, &format!("{PREFIX} <red>Failed to reload MiniMOTD. Ensure there are no errors in your config files. See console for more details."));
            }
        },
        Some("about") => {
            let header = "<st><gradient:white:#007FFF:white>                               </gradient></st>";
            send(sender, header);
            send(
                sender,
                &format!(
                    "<hover:show_text:'<rainbow>click me!'><click:open_url:'https://github.com/jpenilla/MiniMOTD'><white>MiniMOTD <gradient:#0047AB:#007FFF>{} (Rust)",
                    env!("CARGO_PKG_VERSION")
                ),
            );
            send(sender, "<gray>By <white>jmp");
            send(sender, header);
        }
        _ => {
            send(sender, &format!("{PREFIX} <white>MiniMOTD command help"));
            for (cmd, desc) in [("minimotd about", "Show information about MiniMOTD"), ("minimotd reload", "Reload MiniMOTD configuration files"), ("minimotd help", "Show this help menu")] {
                send(sender, &format!("<hover:show_text:'<gray>Click to execute <white>/{cmd}'><click:run_command:'/{cmd}'><gray> - <white>/</white><#007FFF>{cmd}</#007FFF>: <white>{desc}"));
            }
        }
    }
    true
}

fn on_enable(plugin: Plugin) {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(plugin);
    if let Err(e) = reload() {
        plugin.severe(&format!("Failed to load MiniMOTD. Ensure your config files are without errors. {e}"));
    }
    plugin.register_event_with("NORMAL", false, on_ping);
    plugin.set_executor("minimotd", |sender, _, args| command(sender, args));
    plugin.set_tab_completer("minimotd", |sender, _, args| {
        if args.len() <= 1 && sender.has_permission("minimotd.admin") {
            let typed = args.first().copied().unwrap_or("").to_ascii_lowercase();
            ["about", "help", "reload"].iter().filter(|s| s.starts_with(&typed)).map(|s| (*s).to_owned()).collect()
        } else {
            Vec::new()
        }
    });
}

fn on_disable(_plugin: Plugin) {
    *STATE.write().unwrap_or_else(|e| e.into_inner()) = None;
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

bukkit_plugin!(on_enable, on_disable);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modifiers_and_counts() {
        assert_eq!(Modifier::parse("25%").unwrap().apply(4), 5);
        assert_eq!(Modifier::parse("=42").unwrap().apply(4), 42);
        assert_eq!(Modifier::parse("7+").unwrap().apply(4), 7);
        assert_eq!(Modifier::parse("3").unwrap().apply(4), 7);
        let r = Modifier::parse("3:6").unwrap().apply(0);
        assert!((3..6).contains(&r));
        let cfg = MotdConfig::read(&hocon::parse(MAIN_DEFAULT).unwrap());
        assert_eq!(cfg.motds.len(), 2);
        assert_eq!(cfg.counts(100, 20), (69, 69));
        assert!(png_64(&[0x89, b'P', b'N', b'G', 13, 10, 26, 10, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 64, 0, 0, 0, 64, 8]));
        let extra = MotdConfig::read(&hocon::parse(&extra_default("a", "b")).unwrap());
        assert_eq!(extra.motds.len(), 1);
        assert_eq!(extra.motds[0].line1, "a");
    }
}
