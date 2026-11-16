//! A plugin's own resource pack, hosted from the plugin: SHA-1 for the client's check and a small HTTP server (one
//! thread, std only) that hands the zip to anyone who asks. Replaces bundling Netty to serve one file.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, RwLock};


/// A pack's SHA-1 as the 40 hex digits `add_resource_pack` takes.
pub fn sha1_hex(bytes: &[u8]) -> String {
    sha1(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

static SERVING: AtomicBool = AtomicBool::new(false);
static PORT: AtomicU16 = AtomicU16::new(0);
/// What the server hands out: replaced by each `serve`, so a rebuilt pack goes out without a new port.
static BYTES: RwLock<Option<Arc<[u8]>>> = RwLock::new(None);

/// Serves `pack` at `http://<any>:port/` from a thread of its own (one per port); called again, it serves the new
/// bytes from the same thread.
pub fn serve(port: u16, pack: impl Into<Arc<[u8]>>) -> Result<(), String> {
    *BYTES.write().unwrap_or_else(|e| e.into_inner()) = Some(pack.into());
    if SERVING.load(Ordering::Relaxed) && PORT.load(Ordering::Relaxed) == port {
        return Ok(());
    }
    // A plugin hot-swapped in: its old copy lets the port go a moment after it stops.
    let mut tries = 0;
    let listener = loop {
        match std::net::TcpListener::bind(("0.0.0.0", port)) {
            Ok(l) => break l,
            Err(_) if tries < 40 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(e) => return Err(format!("port {port}: {e}")),
        }
    };
    SERVING.store(true, Ordering::Relaxed);
    PORT.store(port, Ordering::Relaxed);
    std::thread::Builder::new()
        .name("kit-pack".into())
        .spawn(move || {
            for stream in listener.incoming() {
                if PORT.load(Ordering::Relaxed) != port {
                    break;
                }
                let Ok(mut stream) = stream else { continue };
                let pack = BYTES.read().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_else(|| Arc::from(&[][..]));
                std::thread::spawn(move || {
                    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
                    let mut head = [0u8; 2048];
                    let n = stream.read(&mut head).unwrap_or(0);
                    let get = head[..n].starts_with(b"GET ") || head[..n].starts_with(b"HEAD ");
                    let body: &[u8] = if head[..n].starts_with(b"GET ") { &pack } else { &[] };
                    let status = if get { "200 OK" } else { "405 Method Not Allowed" };
                    let header = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/zip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        if get { pack.len() } else { 0 }
                    );
                    let _ = stream.write_all(header.as_bytes());
                    let _ = stream.write_all(body);
                });
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Stops serving and lets the port go: the listening thread, blocked waiting for a connection, is woken by one so
/// it drops its listener (a hot-swapped copy of the plugin binds the same port next).
pub fn stop() {
    let port = PORT.swap(0, Ordering::Relaxed);
    SERVING.store(false, Ordering::Relaxed);
    if port != 0 {
        let _ = std::net::TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], port)), std::time::Duration::from_millis(200));
    }
}

// ---------------------------------------------------------------- who has which pack

use horizon_bukkit::Player;
use horizon_bukkit::fam_ev_player::PlayerResourcePackStatusEvent;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

type Then = Box<dyn FnOnce(&Player) + Send>;

struct Tracked {
    url: String,
    sha1: String,
    prompt: Option<String>,
    required: bool,
    loaded: HashSet<String>,
    sent: HashSet<String>,
    waiting: HashMap<String, (std::time::Instant, Then)>,
}

static TRACKED: Mutex<Option<HashMap<String, Tracked>>> = Mutex::new(None);
static LISTENING: AtomicBool = AtomicBool::new(false);

fn key(id: &str) -> String {
    id.replace('-', "").to_ascii_lowercase()
}

/// A pack this plugin's menus need, by its id (32 hex digits, a UUID's): [`has`] tells who loaded it, [`ensure`]
/// sends it to whoever hasn't and runs the menu once it's in.
///
/// ```ignore
/// pack::track(&plugin, "5a1f0c3e2b7d4c919e4a7b3d2f6a8c10", &url, &sha1);   // on_enable
/// pack::restore(&app::folder());                                         // after a reload: who already has it
/// pack::ensure("5a1f...", &player, |p| open_menu(p));                    // opens now, or once the pack is in
/// pack::save(&app::folder());                                            // on_disable
/// ```
pub fn track(plugin: &horizon_bukkit::Plugin, id: &str, url: &str, sha1: &str) {
    let mut t = TRACKED.lock().unwrap_or_else(|e| e.into_inner());
    let all = t.get_or_insert_with(HashMap::new);
    let entry = all.entry(key(id)).or_insert_with(|| Tracked {
        url: String::new(),
        sha1: String::new(),
        prompt: None,
        required: false,
        loaded: HashSet::new(),
        sent: HashSet::new(),
        waiting: HashMap::new(),
    });
    if !entry.sha1.is_empty() && entry.sha1 != sha1.to_ascii_lowercase() {
        // A new pack: nobody has this one yet.
        entry.loaded.clear();
        entry.sent.clear();
    }
    entry.url = url.to_owned();
    entry.sha1 = sha1.to_ascii_lowercase();
    drop(t);
    if !LISTENING.swap(true, Ordering::Relaxed) {
        plugin.register_event_with("MONITOR", false, |e: &mut PlayerResourcePackStatusEvent| status(e));
        plugin.register_event_with("MONITOR", false, |e: &mut horizon_bukkit::event::PlayerQuitEvent| forget(&e.get_player().get_unique_id()));
    }
}

/// The prompt shown with the pack (MiniMessage) and whether refusing it kicks (defaults: none, no).
pub fn options(id: &str, prompt: Option<&str>, required: bool) {
    if let Some(t) = TRACKED.lock().unwrap_or_else(|e| e.into_inner()).as_mut().and_then(|a| a.get_mut(&key(id))) {
        t.prompt = prompt.map(str::to_owned);
        t.required = required;
    }
}

fn status(e: &mut PlayerResourcePackStatusEvent) {
    let id = key(&e.get_id());
    let Some(p) = e.get_player() else { return };
    let uuid = p.get_unique_id();
    let mut t = TRACKED.lock().unwrap_or_else(|e| e.into_inner());
    let Some(tr) = t.as_mut().and_then(|a| a.get_mut(&id)) else { return };
    match e.get_status() {
        "SUCCESSFULLY_LOADED" => {
            tr.loaded.insert(uuid.clone());
            if let Some((at, then)) = tr.waiting.remove(&uuid) {
                drop(t);
                // Only if it came in while they still wanted the menu.
                if at.elapsed().as_secs() < 30 {
                    crate::sync::on_main(move || then(&p));
                }
            }
        }
        "DECLINED" | "FAILED_DOWNLOAD" | "INVALID_URL" | "FAILED_RELOAD" | "DISCARDED" => {
            tr.loaded.remove(&uuid);
            tr.waiting.remove(&uuid);
        }
        _ => {}
    }
}

fn forget(uuid: &str) {
    if let Some(all) = TRACKED.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        for t in all.values_mut() {
            t.loaded.remove(uuid);
            t.sent.remove(uuid);
            t.waiting.remove(uuid);
        }
    }
}

/// Whether the player has the pack loaded.
pub fn has(id: &str, player: &Player) -> bool {
    TRACKED.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|a| a.get(&key(id))).is_some_and(|t| t.loaded.contains(&player.get_unique_id()))
}

/// Runs `then` now if the player has the pack; otherwise sends it (once a session) and runs `then` when it has
/// loaded (within 30 s). Returns whether it ran now.
pub fn ensure(id: &str, player: &Player, then: impl FnOnce(&Player) + Send + 'static) -> bool {
    let uuid = player.get_unique_id();
    let mut t = TRACKED.lock().unwrap_or_else(|e| e.into_inner());
    let Some(tr) = t.as_mut().and_then(|a| a.get_mut(&key(id))) else {
        drop(t);
        then(player);
        return true;
    };
    if tr.loaded.contains(&uuid) {
        drop(t);
        then(player);
        return true;
    }
    tr.waiting.insert(uuid.clone(), (std::time::Instant::now(), Box::new(then)));
    if tr.sent.insert(uuid) {
        let (url, sha, prompt, required) = (tr.url.clone(), tr.sha1.clone(), tr.prompt.clone(), tr.required);
        drop(t);
        let prompt = prompt.map(|m| horizon_bukkit::adventure::Component(crate::mini::parse_mixed(&m)));
        player.add_resource_pack(id, &url, &sha, prompt.as_ref(), required);
    }
    false
}

/// Who has which pack, kept across a reload (a plugin swapped in would otherwise send everyone the pack again,
/// and every client would reload its resources).
pub fn save(folder: &std::path::Path) {
    let t = TRACKED.lock().unwrap_or_else(|e| e.into_inner());
    let Some(all) = t.as_ref() else { return };
    let mut map = serde_json::Map::new();
    for (id, tr) in all {
        map.insert(id.clone(), serde_json::json!({ "sha1": tr.sha1, "loaded": tr.loaded }));
    }
    let _ = std::fs::write(folder.join(".packs-loaded.json"), serde_json::Value::Object(map).to_string());
}

/// After [`track`]: who already had each pack (when its sha1 is the same and they're still online).
pub fn restore(folder: &std::path::Path) {
    let text = std::fs::read_to_string(folder.join(".packs-loaded.json")).unwrap_or_default();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return };
    let online: HashSet<String> = horizon_bukkit::get_online_players().iter().map(|p| p.get_unique_id()).collect();
    let mut t = TRACKED.lock().unwrap_or_else(|e| e.into_inner());
    let Some(all) = t.as_mut() else { return };
    for (id, tr) in all.iter_mut() {
        if v[id.as_str()]["sha1"].as_str() != Some(tr.sha1.as_str()) {
            continue;
        }
        for u in v[id.as_str()]["loaded"].as_array().into_iter().flatten().filter_map(|u| u.as_str()) {
            if online.contains(u) {
                tr.loaded.insert(u.to_owned());
                tr.sent.insert(u.to_owned());
            }
        }
    }
}

/// Marks a player as having a pack (another plugin sent it, or its own file says so).
pub fn mark_loaded(id: &str, uuid: &str) {
    if let Some(t) = TRACKED.lock().unwrap_or_else(|e| e.into_inner()).as_mut().and_then(|a| a.get_mut(&key(id))) {
        t.loaded.insert(uuid.to_owned());
        t.sent.insert(uuid.to_owned());
    }
}

/// SHA-1 (what the client checks the pack against).
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 20];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn sha1_known() {
        let hex = |b: [u8; 20]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        assert_eq!(hex(super::sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(hex(super::sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(super::sha1_hex(b"abc").len(), 40);
    }
}
