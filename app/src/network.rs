//! A network: HorizonX's servers behind one address, through `horizon-proxy` (HorizonsX's Rust proxy, which
//! speaks Velocity's modern forwarding). Players join the proxy's port and land on the lobby; the proxy checks
//! their Minecraft accounts and tells each server who they are with the shared forwarding secret.
//!
//! Everything lives in `<data>/network/`: `network.json` (what the app shows and edits), `proxy.toml` (written
//! from it), `forwarding.secret`, `proxy.pid` and `proxy.log`. A server in the network gets the same
//! `forwarding.secret` and account checks off (the proxy does them); taking it out undoes both.
use crate::{backend, servers, stats};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A server in the network.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Member {
    /// Its name in the proxy (`/server <name>`, the forced hosts): lowercase, no spaces.
    pub name: String,
    pub dir: PathBuf,
    pub port: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Network {
    pub name: String,
    /// The port players join.
    pub port: u16,
    pub motd: String,
    /// Check every player's Minecraft account (off: anyone joins under any name).
    pub online_mode: bool,
    /// The member players land on first.
    pub lobby: String,
    pub members: Vec<Member>,
    /// Where the app reads player counts (127.0.0.1 only).
    pub control_port: u16,
    /// The address players join with (this PC's network address, playit's, or a domain): `/server` sends players
    /// back to it.
    #[serde(default)]
    pub address: String,
}

/// The network plugin's library (`/server`, `/hub`), shipped with the app.
pub const PLUGIN: &str = if cfg!(windows) { "network_rs.dll" } else { "libnetwork_rs.so" };

/// The address players reach this PC at on its own network.
pub fn lan_address() -> String {
    servers::lan_address().map(|a| a.to_string()).unwrap_or_else(|| "localhost".into())
}

pub fn dir() -> PathBuf {
    backend::data_dir().join("network")
}

fn file(name: &str) -> PathBuf {
    dir().join(name)
}

pub fn load() -> Option<Network> {
    fs::read_to_string(file("network.json")).ok().and_then(|t| serde_json::from_str(&t).ok())
}

fn save(net: &Network) -> Result<(), String> {
    fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(net).map_err(|e| e.to_string())?;
    fs::write(file("network.json"), text).map_err(|e| e.to_string())
}

fn beside_app(rel: &str) -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join(rel))
}

/// The proxy program shipped with the app (or `HORIZON_HOST_PROXY`).
pub fn program() -> Option<PathBuf> {
    let name = if cfg!(windows) { "horizon-proxy.exe" } else { "horizon-proxy" };
    std::env::var_os("HORIZON_HOST_PROXY")
        .map(PathBuf::from)
        .or_else(|| beside_app(&format!("engines/horizonsx/{name}")))
        .filter(|p| p.is_file())
}

/// A server's name as the proxy knows it.
pub fn proxy_name(name: &str) -> String {
    let s: String = name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() { "server".into() } else { s }
}

/// The port a server listens on.
pub fn server_port(dir: &Path) -> Option<u16> {
    servers::config(dir).and_then(|c| c["server"]["port"].as_u64()).map(|p| p as u16)
}

fn secret() -> Result<String, String> {
    let path = file("forwarding.secret");
    if let Some(s) = fs::read_to_string(&path).ok().filter(|s| !s.trim().is_empty()) {
        return Ok(s.trim().to_owned());
    }
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    fs::write(&path, &hex).map_err(|e| e.to_string())?;
    Ok(hex)
}

fn toml_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// `proxy.toml` from the network: the proxy reads it when it starts.
fn write_config(net: &Network) -> Result<(), String> {
    fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    fs::write(file("proxy.toml"), config_text(net)).map_err(|e| e.to_string())
}

fn config_text(net: &Network) -> String {
    let mut t = String::new();
    t.push_str("# Written by HorizonX from network.json: change the network in the app (edits here are replaced).\n");
    // Test runs keep the proxy on this PC (no Windows Firewall prompt for each test copy).
    let host = if servers::loopback_only() { "127.0.0.1" } else { "0.0.0.0" };
    t.push_str(&format!("bind = \"{host}:{}\"\n", net.port));
    t.push_str(&format!("motd = {}\n", toml_str(&net.motd)));
    t.push_str("max_players = 10000\n");
    t.push_str(&format!("online_mode = {}\n", net.online_mode));
    t.push_str("workers = 0\nforwarding_secret_file = \"forwarding.secret\"\nbuffer = 32768\n");
    t.push_str(&format!("control = \"127.0.0.1:{}\"\n", net.control_port));
    t.push_str("favicon = \"\"\nversion_name = \"HorizonsX\"\n");
    // The lobby first, then the rest, in the order players are tried when one is down. It goes before any
    // [table]: a key written after one belongs to that table.
    let mut order: Vec<String> = net.members.iter().filter(|m| m.name == net.lobby).map(|m| toml_str(&m.name)).collect();
    order.extend(net.members.iter().filter(|m| m.name != net.lobby).map(|m| toml_str(&m.name)));
    t.push_str(&format!("try = [{}]\n\n[servers]\n", order.join(", ")));
    for m in &net.members {
        t.push_str(&format!("{} = \"127.0.0.1:{}\"\n", toml_str(&m.name), m.port));
    }
    t.push_str("\n[forced_hosts]\n\n[queue]\nenabled = false\n");
    t
}

/// Puts a server behind the proxy: the shared secret beside it, its own account checks off (the proxy does them),
/// and the network plugin with its settings.
fn connect(dir: &Path, secret: &str) -> Result<(), String> {
    fs::write(dir.join("forwarding.secret"), secret).map_err(|e| e.to_string())?;
    servers::set_config(dir, &[("server.online_mode", "false".into())]).map_err(|e| e.message)?;
    if crate::host::bundled_plugins().is_some_and(|f| f.join(PLUGIN).is_file()) {
        crate::host::install_plugin(dir, PLUGIN)?;
    }
    Ok(())
}

/// Takes a server back out: no secret, account checks back on, the network plugin's settings gone.
fn disconnect(dir: &Path) -> Result<(), String> {
    let _ = fs::remove_file(dir.join("forwarding.secret"));
    let _ = fs::remove_file(dir.join("plugins").join("Network").join("config.yml"));
    let _ = crate::host::remove_plugin(dir, PLUGIN);
    servers::set_config(dir, &[("server.online_mode", "true".into())]).map_err(|e| e.message)?;
    Ok(())
}

/// Each server's `plugins/Network/config.yml`: who it is in the network and where players join.
fn write_members(net: &Network) -> Result<(), String> {
    let secret = secret()?;
    let address = if net.address.trim().is_empty() { lan_address() } else { net.address.trim().to_owned() };
    for m in &net.members {
        let folder = m.dir.join("plugins").join("Network");
        fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let text = format!(
            "# Written by HorizonX (Network page): edits here are replaced.\nserver: {}\nlobby: {}\naddress: {}\nport: {}\ncontrol_port: {}\nsecret: {}\n",
            m.name, net.lobby, address, net.port, net.control_port, secret
        );
        fs::write(folder.join("config.yml"), text).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn free_control_port() -> u16 {
    (25600..25700).find(|p| std::net::TcpListener::bind(("127.0.0.1", *p)).is_ok()).unwrap_or(25600)
}

/// Makes the network from these servers (`(name, dir)`), the first being the lobby.
pub fn create(name: &str, port: u16, chosen: &[(String, PathBuf)], lobby: usize) -> Result<Network, String> {
    if chosen.is_empty() {
        return Err("Pick at least one server.".into());
    }
    let secret = secret()?;
    let mut members = Vec::new();
    for (n, d) in chosen {
        let port = server_port(d).ok_or(format!("{n}'s settings couldn't be read."))?;
        let mut pname = proxy_name(n);
        while members.iter().any(|m: &Member| m.name == pname) {
            pname.push('2');
        }
        members.push(Member { name: pname, dir: d.clone(), port });
    }
    let lobby = members.get(lobby).or(members.first()).map(|m| m.name.clone()).unwrap_or_default();
    let net = Network {
        name: name.trim().to_owned(),
        port,
        motd: format!("{} network", name.trim()),
        online_mode: true,
        lobby,
        members,
        control_port: free_control_port(),
        address: String::new(),
    };
    for m in &net.members {
        connect(&m.dir, &secret)?;
    }
    write_config(&net)?;
    write_members(&net)?;
    save(&net)?;
    Ok(net)
}

/// Saves a changed network (name, MOTD, account checks, lobby, port) and rewrites the proxy's settings.
pub fn update(net: &Network) -> Result<(), String> {
    write_config(net)?;
    write_members(net)?;
    save(net)
}

/// Adds a server to the network.
pub fn add(name: &str, dir_: &Path) -> Result<Network, String> {
    let mut net = load().ok_or("There is no network yet.")?;
    if net.members.iter().any(|m| m.dir == dir_) {
        return Ok(net);
    }
    let port = server_port(dir_).ok_or(format!("{name}'s settings couldn't be read."))?;
    connect(dir_, &secret()?)?;
    let mut pname = proxy_name(name);
    while net.members.iter().any(|m| m.name == pname) {
        pname.push('2');
    }
    net.members.push(Member { name: pname, dir: dir_.to_path_buf(), port });
    if net.lobby.is_empty() {
        net.lobby = net.members[0].name.clone();
    }
    update(&net)?;
    Ok(net)
}

/// Takes a server out of the network.
pub fn remove(dir_: &Path) -> Result<Network, String> {
    let mut net = load().ok_or("There is no network yet.")?;
    disconnect(dir_)?;
    net.members.retain(|m| m.dir != dir_);
    if !net.members.iter().any(|m| m.name == net.lobby) {
        net.lobby = net.members.first().map(|m| m.name.clone()).unwrap_or_default();
    }
    update(&net)?;
    Ok(net)
}

/// Takes every server out and deletes the network's settings (the proxy stops first).
pub fn delete() -> Result<(), String> {
    let _ = stop();
    if let Some(net) = load() {
        for m in &net.members {
            let _ = disconnect(&m.dir);
        }
    }
    fs::remove_dir_all(dir()).map_err(|e| e.to_string())
}

// ----- The proxy's process -----------------------------------------------------------------------

/// The proxy's process id while it runs.
pub fn pid() -> Option<u32> {
    let pid: u32 = fs::read_to_string(file("proxy.pid")).ok()?.trim().parse().ok()?;
    stats::process_memory(pid).map(|_| pid)
}

pub fn running() -> bool {
    pid().is_some()
}

/// Its memory now.
pub fn memory() -> Option<u64> {
    pid().and_then(stats::process_memory)
}

/// Starts the proxy in the background; it keeps running when the app closes, like the servers.
pub fn start() -> Result<u32, String> {
    if let Some(pid) = pid() {
        return Ok(pid);
    }
    let program = program().ok_or("The network proxy (horizon-proxy) isn't beside the app: reinstall HorizonX.")?;
    let net = load().ok_or("There is no network yet.")?;
    if net.members.is_empty() {
        return Err("Add a server to the network first.".into());
    }
    write_config(&net)?;
    let log = fs::OpenOptions::new().create(true).append(true).open(file("proxy.log")).map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(program);
    cmd.arg("proxy.toml").current_dir(dir()).stdin(std::process::Stdio::null()).stdout(log.try_clone().map_err(|e| e.to_string())?).stderr(log);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window, and not ended with the app.
        cmd.creation_flags(0x0800_0000 | 0x0000_0008);
    }
    let child = cmd.spawn().map_err(|e| format!("The proxy didn't start: {e}"))?;
    fs::write(file("proxy.pid"), child.id().to_string()).map_err(|e| e.to_string())?;
    // A bad setting ends it at once: say so instead of "running".
    std::thread::sleep(Duration::from_millis(700));
    if pid().is_none() {
        let tail = log_tail(4).join(" ");
        return Err(format!("The proxy stopped right away. {tail}"));
    }
    Ok(child.id())
}

/// Stops the proxy (players on it are disconnected).
pub fn stop() -> Result<(), String> {
    let Some(pid) = pid() else { return Ok(()) };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("taskkill").args(["/PID", &pid.to_string(), "/T", "/F"]).creation_flags(0x0800_0000).output().map_err(|e| e.to_string())?;
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("kill").arg(pid.to_string()).output().map_err(|e| e.to_string())?;
    }
    // A killed proxy can take a few seconds to be gone (its sockets close first).
    for _ in 0..70 {
        if stats::process_memory(pid).is_none() {
            let _ = fs::remove_file(file("proxy.pid"));
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    Err("The proxy didn't stop.".into())
}

/// Stops and starts the proxy (it reads its settings again).
pub fn restart() -> Result<u32, String> {
    stop()?;
    start()
}

/// Players on the network: in all, and by server name. None while the proxy is down.
pub fn counts() -> Option<(u64, Vec<(String, u64)>)> {
    let net = load()?;
    let secret = fs::read_to_string(file("forwarding.secret")).ok()?;
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], net.control_port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(400)).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(800))).ok()?;
    s.write_all(format!("{} COUNTS\n", secret.trim()).as_bytes()).ok()?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).ok()?;
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    let total = v["total"].as_u64().or_else(|| v["all"].as_u64()).unwrap_or(0);
    let by = v["servers"].as_object().or_else(|| v.as_object()).map(|o| {
        o.iter().filter(|(k, _)| *k != "total" && *k != "all").filter_map(|(k, n)| n.as_u64().map(|n| (k.clone(), n))).collect()
    });
    Some((total, by.unwrap_or_default()))
}

/// The proxy's newest log lines.
pub fn log_tail(n: usize) -> Vec<String> {
    let text = fs::read_to_string(file("proxy.log")).unwrap_or_default();
    let lines: Vec<String> = text.lines().filter(|l| !l.trim().is_empty()).map(str::to_owned).collect();
    lines[lines.len().saturating_sub(n)..].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_settings_put_try_at_the_top() {
        let m = |n: &str, p| Member { name: n.into(), dir: PathBuf::from(n), port: p };
        let net = Network {
            name: "Test".into(),
            port: 25577,
            motd: "A \"test\" network".into(),
            online_mode: true,
            lobby: "hub".into(),
            members: vec![m("survival", 25566), m("hub", 25565)],
            control_port: 25600,
            address: String::new(),
        };
        let v: toml::Value = toml::from_str(&config_text(&net)).expect("valid TOML");
        let order: Vec<&str> = v["try"].as_array().expect("try at the top level").iter().filter_map(|x| x.as_str()).collect();
        assert_eq!(order, ["hub", "survival"]);
        assert_eq!(v["servers"]["survival"].as_str(), Some("127.0.0.1:25566"));
        assert_eq!(v["motd"].as_str(), Some("A \"test\" network"));
        assert_eq!(v["control"].as_str(), Some("127.0.0.1:25600"));
    }
}
