//! `horizon-proxy`: one address in front of many HorizonsX servers, as a network's proxy (Velocity's job), in Rust.
//!
//! What makes it light: after the login handoff it never looks at a packet again. The player is told to use the
//! same compression threshold the server chose, so the server's compressed frames (HorizonsX compresses each chunk
//! once for every viewer) go to the player byte for byte, and the player's back to the server: a byte copy in each
//! direction (with online mode, AES-CFB8 on the player's side, on the CPU's AES instructions), no unpacking,
//! recompressing or re-framing. A Java Velocity in front of three HorizonsX servers on one machine used 9.5 cores and
//! 2.5 GB for 600 players doing exactly that unpacking and repacking; this, 1.8 cores and 47 MB (11-06).
//!
//! The handoff: the proxy reads the player's handshake and login; in online mode it asks for encryption, checks the
//! sign-in with Mojang's session server (`hasJoined`) and takes the player's real UUID, name and skin; then it picks a
//! server (the forced host for the address the player typed, else the first of `try`), logs in to it as the player,
//! answers its `velocity:player_info` query with Velocity's modern forwarding (HMAC-SHA256 with the shared secret:
//! version 1, address, UUID, name, profile properties: what HorizonsX's `lan/forwarding.rs` checks), and relays its
//! compression and login answer. The server list ping is answered by the proxy itself.
//!
//! Moving between servers: a server's plugin tells the proxy where a player goes next (the control port: `SEND
//! <player> <server>`) and sends the player the game's own Transfer packet back to the proxy's address; the proxy
//! routes that reconnect to the server asked for. The control port also answers `COUNTS` (players per server).
//!
//! The join queue (`[queue]`, queue.rs): the game servers (never the hub) have a line. A move there that can't happen
//! at once gets a place; the player waits in the hub (whose plugin shows the place) and goes when their turn comes,
//! at a safe rate (network-wide and per server, held while a server reports trouble), by tier.
//!
//! Usage: horizon-proxy [CONFIG]   (default proxy.toml; written with defaults when missing)

mod limbo;
mod queue;
mod screen;

use aes::cipher::{BlockEncrypt, KeyInit, generic_array::GenericArray};
use horizon_protocol::{
    buf::{Reader, write_varint},
    crypto::{offline_uuid, random_bytes, server_hash},
    frame,
    packets::{configuration as cfg, handshake, login, play, status},
    rsa::RsaPrivateKey,
};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};

const DEFAULT_CONFIG: &str = r##"# horizon-proxy: one address in front of many HorizonsX servers.
bind = "0.0.0.0:25577"
motd = "A HorizonsX network"
max_players = 10000
# Check every player's Mojang sign-in (and encrypt their connection). false: anyone joins under any name.
online_mode = true
# Threads that move the bytes (0: four, or the cores if fewer; raise for thousands of players).
workers = 0
# The secret the servers have in their forwarding.secret file.
forwarding_secret_file = "forwarding.secret"
# Bytes each direction of a connection buffers (the copy's chunk).
buffer = 32768

[servers]
lobby = "127.0.0.1:25566"

# The order a player who typed no forced host is sent through (the next is tried when one is down).
try = ["lobby"]

# Where the servers' plugins ask the proxy to move players and read player counts (keep it on 127.0.0.1;
# every request starts with the forwarding secret). Empty: off.
control = "127.0.0.1:25600"
# The server list: motd may be text or a text component as JSON; favicon a 64x64 PNG beside this file.
favicon = ""
version_name = "HorizonsX"

# The address a player typed -> the server they join.
[forced_hosts]
# "lobby.example.com" = "lobby"

# The join queue for the game servers (never the hub): players wait in the hub and move over at a safe rate.
[queue]
enabled = false
queued = ["survival"]
max_size = 5000
# Turns across the network: `admit` players every `every_seconds` (spread out; at most `burst` at once).
admit = 10
every_seconds = 10
burst = 2
# For the first minutes after the proxy starts: gentler.
startup_minutes = 5
startup_admit = 4
startup_every_seconds = 10
# A player who leaves the network keeps their place this long (also across a proxy restart).
grace_seconds = 300
ticket_seconds = 60
always_queue = false
state_file = "queue-state.json"
# LuckPerms JSON storage folders: players' groups and permissions pick their tier.
luckperms = []
# The queue screen: seconds between updates; the store for "Get Priority" (empty: no button); the address players
# are sent on through (empty: the one they typed).
refresh_seconds = 2
store_url = ""
transfer_host = ""
transfer_port = 0

# Each queued server: its name on screen, a cap, its own rate, and the health it must report to take players.
[queue.servers.survival]
display = "Survival"
max_players = 100
admit = 2
every_seconds = 10
min_tps = 17
max_mspt = 45
max_memory_percent = 92
health_timeout_seconds = 20

# Tiers, best first. bypass = no line. weight = how fast the line moves against the others.
[[queue.tiers]]
name = "staff"
display = "Staff"
color = "#f87171"
bypass = true
groups = ["owner", "admin"]

[[queue.tiers]]
name = "standard"
display = "Standard"
color = "#a1a1aa"
weight = 1
"##;

/// Mojang's session server: whether a player who says they signed in did (`hasJoined`).
const SESSION_SERVER: &str = "https://sessionserver.mojang.com/session/minecraft/hasJoined";

#[derive(serde::Deserialize)]
struct Config {
    bind: String,
    #[serde(default = "default_motd")]
    motd: String,
    #[serde(default = "default_max")]
    max_players: usize,
    #[serde(default = "default_true")]
    online_mode: bool,
    #[serde(default)]
    workers: usize,
    #[serde(default = "default_secret_file")]
    forwarding_secret_file: String,
    #[serde(default = "default_buffer")]
    buffer: usize,
    servers: HashMap<String, String>,
    #[serde(rename = "try", default)]
    try_order: Vec<String>,
    #[serde(default)]
    forced_hosts: HashMap<String, String>,
    #[serde(default)]
    control: String,
    #[serde(default)]
    favicon: String,
    #[serde(default = "default_version")]
    version_name: String,
    #[serde(default)]
    queue: queue::QueueConfig,
}
fn default_version() -> String {
    "HorizonsX".into()
}
fn default_motd() -> String {
    "A HorizonsX network".into()
}
fn default_max() -> usize {
    10000
}
fn default_true() -> bool {
    true
}
fn default_secret_file() -> String {
    "forwarding.secret".into()
}
fn default_buffer() -> usize {
    32768
}

struct Proxy {
    config: Config,
    secret: Vec<u8>,
    online: AtomicUsize,
    /// Players on each server.
    counts: HashMap<String, AtomicUsize>,
    /// Where a player's next login goes (by UUID as 32 hex digits, or lower-case name), asked by a server's plugin.
    pending: std::sync::Mutex<HashMap<String, (String, std::time::Instant)>>,
    /// The server list's icon as a data URL.
    favicon: Option<String>,
    /// The proxy's own key for online-mode logins (made at each start, as vanilla's server does).
    key: Option<(RsaPrivateKey, Vec<u8>)>,
    http: ureq::Agent,
    queue: std::sync::Mutex<queue::Queue>,
    /// Where the queue is saved.
    queue_file: std::path::PathBuf,
    /// Open connections per player (a move between servers overlaps the old one briefly).
    sessions: std::sync::Mutex<HashMap<u128, usize>>,
    /// The queue world's recording (None: the queue screen).
    limbo: Option<limbo::Limbo>,
}

impl Proxy {
    fn queue(&self) -> std::sync::MutexGuard<'_, queue::Queue> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// Whether the player has another connection open (the one being dropped is still counted: more than one).
    fn connected_elsewhere(&self, uuid: u128) -> bool {
        self.sessions.lock().unwrap_or_else(|e| e.into_inner()).get(&uuid).is_some_and(|n| *n > 1)
    }
    /// Players on each server now.
    fn count_map(&self) -> HashMap<String, usize> {
        self.counts.iter().map(|(k, v)| (k.clone(), v.load(Ordering::Relaxed))).collect()
    }
}

type Result<T> = std::result::Result<T, String>;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "proxy.toml".into());
    if !std::path::Path::new(&path).exists() {
        std::fs::write(&path, DEFAULT_CONFIG).unwrap_or_else(|e| panic!("writing {path}: {e}"));
        println!("wrote {path} with the defaults: set the servers and run again");
    }
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    let config: Config = toml::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"));
    let secret_path = std::path::Path::new(&path).with_file_name(&config.forwarding_secret_file);
    if !secret_path.exists() {
        let mut bytes = [0u8; 16];
        random_bytes(&mut bytes).expect("the system's random numbers");
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        std::fs::write(&secret_path, &hex).expect("writing the forwarding secret");
        println!("wrote a new forwarding secret to {}: copy it into each server's forwarding.secret", secret_path.display());
    }
    let secret = std::fs::read_to_string(&secret_path).expect("reading the forwarding secret").trim().as_bytes().to_vec();
    if secret.is_empty() {
        panic!("{} is empty", secret_path.display());
    }
    let key = config.online_mode.then(|| {
        let key = RsaPrivateKey::generate(1024).expect("an RSA key for online mode");
        let der = key.public_der();
        (key, der)
    });
    if !config.online_mode {
        println!("online_mode = false: players are not checked with Mojang; keep this proxy on a private network");
    }
    // 0: four threads (or the cores, if fewer). A thread a core only spent more on handing connections between
    // threads: 900 players through 3 servers carried the same data (chunks delivered within noise) with 2 threads
    // at 1.38 cores and 8 at 1.70; 2 ran at ~70% busy there, so 4 keeps room to grow. Large networks set more.
    let workers = if config.workers == 0 {
        std::thread::available_parallelism().map_or(4, |n| n.get()).min(4)
    } else {
        config.workers
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .thread_name("horizon-proxy")
        .enable_io()
        .enable_time()
        .build()
        .expect("the proxy's threads start");
    let http = ureq::AgentBuilder::new().timeout(Duration::from_secs(10)).build();
    let counts = config.servers.keys().map(|k| (k.clone(), AtomicUsize::new(0))).collect();
    let favicon = (!config.favicon.is_empty())
        .then(|| std::fs::read(std::path::Path::new(&path).with_file_name(&config.favicon)).ok())
        .flatten()
        .map(|png| format!("data:image/png;base64,{}", base64(&png)));
    let queue_file = std::path::Path::new(&path).with_file_name(&config.queue.state_file);
    // The queue world's recording, next to the config (none or unusable: the queue screen).
    let limbo = (config.queue.enabled && !config.queue.world_file.is_empty())
        .then(|| limbo::Limbo::load(&std::path::Path::new(&path).with_file_name(&config.queue.world_file)))
        .flatten();
    let mut q = queue::Queue::new(config.queue.clone());
    if config.queue.enabled
        && let Ok(text) = std::fs::read_to_string(&queue_file)
    {
        let n = q.load_json(&text);
        if n > 0 {
            println!("queue: {n} players had a place before the restart; it is kept for {} s", config.queue.grace_seconds);
        }
    }
    let proxy = Arc::new(Proxy { config, secret, online: AtomicUsize::new(0), counts, pending: Default::default(), favicon, key, http, queue: std::sync::Mutex::new(q), queue_file, sessions: Default::default(), limbo });
    if !proxy.config.control.is_empty() {
        let p = proxy.clone();
        runtime.spawn(async move { control(p).await });
    }
    if proxy.config.queue.enabled {
        let p = proxy.clone();
        runtime.spawn(async move { admissions(p).await });
    }
    runtime.block_on(serve(proxy));
}

async fn serve(proxy: Arc<Proxy>) {
    let listener = TcpListener::bind(&proxy.config.bind)
        .await
        .unwrap_or_else(|e| panic!("listening on {}: {e}", proxy.config.bind));
    println!(
        "horizon-proxy listening on {} in front of {} servers ({} threads, online mode {}); Done",
        proxy.config.bind,
        proxy.config.servers.len(),
        tokio::runtime::Handle::current().metrics().num_workers(),
        proxy.config.online_mode
    );
    loop {
        let Ok((client, address)) = listener.accept().await else {
            continue;
        };
        let proxy = proxy.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(&proxy, client, address).await
                && !e.is_empty()
            {
                eprintln!("{address}: {e}");
            }
        });
    }
}

// ---- AES-CFB8 (the protocol's cipher), on the CPU's AES instructions ----

struct Cfb8 {
    aes: aes::Aes128,
    iv: [u8; 16],
}
impl Cfb8 {
    fn new(secret: &[u8; 16]) -> Self {
        Self { aes: aes::Aes128::new(GenericArray::from_slice(secret)), iv: *secret }
    }
    fn encrypt(&mut self, data: &mut [u8]) {
        for b in data {
            let mut block = GenericArray::from(self.iv);
            self.aes.encrypt_block(&mut block);
            *b ^= block[0];
            self.iv.copy_within(1.., 0);
            self.iv[15] = *b;
        }
    }
    fn decrypt(&mut self, data: &mut [u8]) {
        for b in data {
            let mut block = GenericArray::from(self.iv);
            self.aes.encrypt_block(&mut block);
            let c = *b;
            *b ^= block[0];
            self.iv.copy_within(1.., 0);
            self.iv[15] = c;
        }
    }
}

// ---- Frames ----

/// One frame's contents (after its length), read off `r`.
async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> Result<Vec<u8>> {
    let mut len = 0usize;
    for i in 0..3 {
        let b = r.read_u8().await.map_err(|_| String::new())?;
        len |= usize::from(b & 0x7F) << (7 * i);
        if b & 0x80 == 0 {
            if len == 0 || len > frame::MAX_FRAME {
                return Err("bad frame length".into());
            }
            let mut body = vec![0u8; len];
            r.read_exact(&mut body).await.map_err(|_| String::new())?;
            return Ok(body);
        }
    }
    Err("frame length wider than 21 bits".into())
}

/// A frame's packet: its id and payload (`threshold`: the compression in force).
fn packet_of(contents: Vec<u8>, threshold: Option<usize>) -> Result<(i32, Vec<u8>)> {
    let body = frame::decompress(contents, threshold).map_err(|e| e.to_string())?;
    let mut r = Reader::new(&body);
    let id = r.varint().map_err(|e| e.to_string())?;
    let at = r.position();
    Ok((id, body[at..].to_vec()))
}

/// A packet body (id + payload) as a frame under `threshold`.
fn frame_of(body: &[u8], threshold: Option<usize>) -> Result<Vec<u8>> {
    frame::encode(body, threshold).map_err(|e| e.to_string())
}

async fn send<W: AsyncWrite + Unpin>(w: &mut W, bytes: &[u8]) -> Result<()> {
    w.write_all(bytes).await.map_err(|_| String::new())
}

fn disconnect_json(text: &str) -> String {
    serde_json::json!({ "text": text }).to_string()
}

/// The player's side of a connection: frames read and bytes written through the cipher once online mode turns it on.
struct Client {
    stream: TcpStream,
    /// Bytes read (and decrypted) but not taken yet.
    buf: Vec<u8>,
    dec: Option<Cfb8>,
    enc: Option<Cfb8>,
}
impl Client {
    async fn read_frame(&mut self) -> Result<Vec<u8>> {
        loop {
            if let Some(f) = frame::split(&mut self.buf).map_err(|e| e.to_string())? {
                return Ok(f);
            }
            let mut chunk = [0u8; 4096];
            let n = self.stream.read(&mut chunk).await.map_err(|_| String::new())?;
            if n == 0 {
                return Err(String::new());
            }
            if let Some(d) = self.dec.as_mut() {
                d.decrypt(&mut chunk[..n]);
            }
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }
    async fn send(&mut self, bytes: &[u8]) -> Result<()> {
        match self.enc.as_mut() {
            Some(e) => {
                let mut out = bytes.to_vec();
                e.encrypt(&mut out);
                send(&mut self.stream, &out).await
            }
            None => send(&mut self.stream, bytes).await,
        }
    }
}

// ---- A connection ----

async fn handle(proxy: &Proxy, stream: TcpStream, address: SocketAddr) -> Result<()> {
    let _ = stream.set_nodelay(true);
    let mut client = Client { stream, buf: Vec::new(), dec: None, enc: None };
    let (id, payload) = packet_of(client.read_frame().await?, None)?;
    let handshake::Serverbound::Intention(intent) =
        handshake::Serverbound::decode(id, &payload).map_err(|e| e.to_string())?
    else {
        return Err("expected a handshake".into());
    };
    match intent.intent {
        handshake::STATUS => status_reply(proxy, &mut client, intent.protocol).await,
        handshake::LOGIN | handshake::TRANSFER => login(proxy, client, address, intent).await,
        other => Err(format!("unknown intent {other}")),
    }
}

/// The server list: the proxy's own answer (the network's players, its MOTD), and the ping's pong.
async fn status_reply(proxy: &Proxy, client: &mut Client, protocol: i32) -> Result<()> {
    loop {
        let Ok(contents) = client.read_frame().await else {
            return Ok(());
        };
        let (id, payload) = packet_of(contents, None)?;
        match status::Serverbound::decode(id, &payload).map_err(|e| e.to_string())? {
            status::Serverbound::StatusRequest(_) => {
                let motd = proxy.config.motd.trim();
                let description = if motd.starts_with('{') || motd.starts_with('[') {
                    serde_json::from_str(motd).unwrap_or_else(|_| serde_json::json!({ "text": motd }))
                } else {
                    serde_json::json!({ "text": motd })
                };
                let mut v = serde_json::json!({
                    "version": { "name": proxy.config.version_name, "protocol": protocol },
                    "players": { "max": proxy.config.max_players, "online": proxy.online.load(Ordering::Relaxed), "sample": queue_sample(proxy) },
                    "description": description,
                });
                if let Some(icon) = &proxy.favicon {
                    v["favicon"] = serde_json::json!(icon);
                }
                let json = v.to_string();
                let body = status::Clientbound::StatusResponse(status::StatusResponse { json }).encode();
                client.send(&frame_of(&body, None)?).await?;
            }
            status::Serverbound::PingRequest(ping) => {
                let body = status::Clientbound::PongResponse(ping).encode();
                client.send(&frame_of(&body, None)?).await?;
                return Ok(());
            }
            status::Serverbound::Unknown(..) => return Ok(()),
        }
    }
}

/// The servers to try for a login, in order: the one a plugin asked for (`SEND`), the forced host for the address
/// typed, then `try`.
fn candidates<'a>(proxy: &'a Proxy, host: &str, uuid: u128, name: &str, first: Option<&str>) -> Vec<(&'a str, &'a str)> {
    let mut out: Vec<(&str, &str)> = Vec::new();
    if let Some(f) = first
        && let Some((k, a)) = proxy.config.servers.get_key_value(f)
    {
        out.push((k.as_str(), a.as_str()));
    }
    let asked = {
        let mut pending = proxy.pending.lock().unwrap_or_else(|e| e.into_inner());
        pending.retain(|_, (_, at)| at.elapsed() < Duration::from_secs(60));
        pending.remove(&format!("{uuid:032x}")).or_else(|| pending.remove(&name.to_ascii_lowercase()))
    };
    if let Some((server, _)) = asked
        && let Some((k, a)) = proxy.config.servers.get_key_value(&server)
    {
        out.push((k.as_str(), a.as_str()));
    }
    if let Some(first) = pick_server(proxy, host) {
        out.push(first);
    }
    for n in &proxy.config.try_order {
        if let Some((k, a)) = proxy.config.servers.get_key_value(n) {
            out.push((k.as_str(), a.as_str()));
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|(k, _)| seen.insert(*k));
    out
}

/// The server a player joins: the forced host for the address they typed, else the first of `try` that exists.
fn pick_server<'a>(proxy: &'a Proxy, host: &str) -> Option<(&'a str, &'a str)> {
    // A modded client appends "\0FML..." to the host; a trailing dot is the DNS root.
    let host = host.split('\0').next().unwrap_or(host).trim_end_matches('.').to_ascii_lowercase();
    let name = proxy
        .config
        .forced_hosts
        .get(&host)
        .or_else(|| proxy.config.try_order.iter().find(|n| proxy.config.servers.contains_key(*n)))?;
    let (key, addr) = proxy.config.servers.get_key_value(name)?;
    Some((key.as_str(), addr.as_str()))
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    write_varint(out, s.len() as i32);
    out.extend_from_slice(s.as_bytes());
}

/// Velocity's modern forwarding answer: HMAC-SHA256 of the data, then the data (version 1: address, UUID, name,
/// profile properties: each a name, a value, and its signature when there is one).
fn forwarding_payload(secret: &[u8], address: &SocketAddr, uuid: u128, name: &str, properties: &[login::Property]) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    let mut data = Vec::with_capacity(64);
    write_varint(&mut data, 1);
    write_string(&mut data, &address.ip().to_string());
    data.extend_from_slice(&uuid.to_be_bytes());
    write_string(&mut data, name);
    write_varint(&mut data, properties.len() as i32);
    for p in properties {
        write_string(&mut data, &p.name);
        write_string(&mut data, &p.value);
        data.push(u8::from(p.signature.is_some()));
        if let Some(s) = &p.signature {
            write_string(&mut data, s);
        }
    }
    let mut mac = <Hmac<sha2::Sha256> as Mac>::new_from_slice(secret).expect("any key length");
    mac.update(&data);
    let mut out = mac.finalize().into_bytes().to_vec();
    out.extend_from_slice(&data);
    out
}

/// Mojang's answer to `hasJoined`: the player's UUID, name and profile properties (the skin), or None.
fn has_joined(http: &ureq::Agent, name: &str, hash: &str) -> Result<Option<(u128, String, Vec<login::Property>)>> {
    let url = format!("{SESSION_SERVER}?username={name}&serverId={hash}");
    let response = match http.get(&url).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(204 | 403 | 404, _)) => return Ok(None),
        Err(e) => return Err(format!("the session server did not answer: {e}")),
    };
    if response.status() == 204 {
        return Ok(None);
    }
    let body = response.into_string().map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let Some(id) = v["id"].as_str() else {
        return Ok(None);
    };
    let uuid = u128::from_str_radix(id, 16).map_err(|e| e.to_string())?;
    let name = v["name"].as_str().unwrap_or(name).to_owned();
    let properties = v["properties"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    Some(login::Property {
                        name: p["name"].as_str()?.to_owned(),
                        value: p["value"].as_str()?.to_owned(),
                        signature: p["signature"].as_str().map(str::to_owned),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Some((uuid, name, properties)))
}

fn kick_frame(text: &str) -> Vec<u8> {
    let body = login::Clientbound::Disconnect(login::Disconnect { reason_json: disconnect_json(text) }).encode();
    frame_of(&body, None).unwrap_or_default()
}

/// Online mode: the encryption request, the player's answer, Mojang's check. The player's UUID, name and skin, with
/// the connection now encrypted both ways; None after kicking a player who didn't sign in.
async fn sign_in(proxy: &Proxy, client: &mut Client, name: &str) -> Result<Option<(u128, String, Vec<login::Property>)>> {
    let (key, der) = proxy.key.as_ref().expect("online mode has a key");
    let mut challenge = [0u8; 4];
    random_bytes(&mut challenge).map_err(|e| e.to_string())?;
    let request = login::Clientbound::Hello(login::Hello {
        server_id: String::new(),
        public_key: der.clone(),
        challenge: challenge.to_vec(),
        should_authenticate: true,
    })
    .encode();
    client.send(&frame_of(&request, None)?).await?;
    let (id, payload) = packet_of(client.read_frame().await?, None)?;
    let login::Serverbound::Key(answer) = login::Serverbound::decode(id, &payload).map_err(|e| e.to_string())? else {
        return Err("expected the encryption answer".into());
    };
    let shared = key.decrypt(&answer.key).map_err(|e| e.to_string())?;
    let echoed = key.decrypt(&answer.challenge).map_err(|e| e.to_string())?;
    if echoed != challenge || shared.len() != 16 {
        let _ = client.send(&kick_frame("Encryption failed")).await;
        return Ok(None);
    }
    let shared: [u8; 16] = shared.try_into().expect("16 bytes");
    // From here every byte each way goes through the cipher; what was read past the answer is decrypted now.
    client.enc = Some(Cfb8::new(&shared));
    let mut dec = Cfb8::new(&shared);
    dec.decrypt(&mut client.buf);
    client.dec = Some(dec);
    let hash = server_hash("", &shared, der);
    let http = proxy.http.clone();
    let asked = name.to_owned();
    let joined = tokio::task::spawn_blocking(move || has_joined(&http, &asked, &hash))
        .await
        .map_err(|e| e.to_string())?;
    match joined {
        Ok(Some(profile)) => Ok(Some(profile)),
        Ok(None) => {
            let _ = client.send(&kick_frame("Failed to verify username!")).await;
            Ok(None)
        }
        Err(e) => {
            let _ = client.send(&kick_frame("Authentication servers are down. Please try again later, sorry!")).await;
            Err(e)
        }
    }
}

async fn login(proxy: &Proxy, mut client: Client, address: SocketAddr, intent: handshake::Intention) -> Result<()> {
    let (id, payload) = packet_of(client.read_frame().await?, None)?;
    let login::Serverbound::Hello(hello) = login::Serverbound::decode(id, &payload).map_err(|e| e.to_string())? else {
        return Err("expected Login Start".into());
    };
    let (uuid, name, properties) = if proxy.config.online_mode {
        match sign_in(proxy, &mut client, &hello.name).await? {
            Some(profile) => profile,
            None => return Ok(()),
        }
    } else {
        (offline_uuid(&hello.name), hello.name.clone(), Vec::new())
    };
    // An admitted player's ticket takes them to their server; a player who had a place in a line has it again.
    let first: Option<String> = if proxy.config.queue.enabled {
        let (ticket, has_place) = {
            let mut q = proxy.queue();
            let had = q.arrived(uuid);
            (q.take_ticket(uuid), had)
        };
        // A place in a line (just given by the hub, or kept from before): the queue screen until their turn.
        if ticket.is_none() && has_place {
            return wait_in_queue(proxy, client, &intent, uuid, name, properties).await;
        }
        ticket
    } else {
        None
    };
    let mut chosen = None;
    for (server_name, server_addr) in candidates(proxy, &intent.host, uuid, &name, first.as_deref()) {
        if let Ok(Ok(s)) = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(server_addr)).await {
            chosen = Some((server_name, server_addr, s));
            break;
        }
        eprintln!("{name}: {server_name} ({server_addr}) is not reachable, trying the next");
    }
    let Some((server_name, server_addr, backend)) = chosen else {
        let _ = client.send(&kick_frame("The network is restarting: try again in a minute")).await;
        return Ok(());
    };
    let _ = backend.set_nodelay(true);
    let mut backend = BufReader::with_capacity(16384, backend);
    // As the player: handshake for login, then Login Start.
    let (host, port) = server_addr.rsplit_once(':').unwrap_or((server_addr, "25565"));
    let hs = handshake::Serverbound::Intention(handshake::Intention {
        protocol: intent.protocol,
        host: host.to_owned(),
        port: port.parse().unwrap_or(25565),
        intent: handshake::LOGIN,
    })
    .encode();
    let start = login::Serverbound::Hello(login::ServerHello { name: name.clone(), uuid }).encode();
    let mut out = frame_of(&hs, None)?;
    out.extend(frame_of(&start, None)?);
    send(backend.get_mut(), &out).await?;

    // The server's login: forwarding query, compression, then its answer, relayed.
    let mut threshold: Option<usize> = None;
    loop {
        let contents = read_frame(&mut backend).await.map_err(|_| format!("{server_name} closed during login"))?;
        let (id, payload) = packet_of(contents.clone(), threshold)?;
        match login::Clientbound::decode(id, &payload).map_err(|e| e.to_string())? {
            login::Clientbound::CustomQuery(q) => {
                let answer = (q.channel == "velocity:player_info")
                    .then(|| forwarding_payload(&proxy.secret, &address, uuid, &name, &properties));
                let body = login::Serverbound::CustomQueryAnswer(login::CustomQueryAnswer { transaction: q.transaction, payload: answer }).encode();
                send(backend.get_mut(), &frame_of(&body, threshold)?).await?;
            }
            login::Clientbound::Compression(c) => {
                // The player is told the same threshold, so the server's frames pass through as they are.
                client.send(&frame_of(&login::Clientbound::Compression(c.clone()).encode(), None)?).await?;
                threshold = usize::try_from(c.threshold).ok();
            }
            login::Clientbound::LoginFinished(_) => {
                client.send(&frame::prefixed(&contents)).await?;
                break;
            }
            login::Clientbound::Disconnect(_) => {
                client.send(&frame::prefixed(&contents)).await?;
                return Ok(());
            }
            login::Clientbound::Hello(_) => {
                let _ = client.send(&kick_frame(&format!("{server_name} is in online mode: it must trust the proxy"))).await;
                return Ok(());
            }
            login::Clientbound::CookieRequest(_) | login::Clientbound::Unknown(..) => {
                client.send(&frame::prefixed(&contents)).await?;
            }
        }
    }

    // Passthrough: what either side had buffered first, then the bytes each way until one closes.
    proxy.online.fetch_add(1, Ordering::Relaxed);
    struct Online<'a>(&'a AtomicUsize, Option<&'a AtomicUsize>);
    impl Drop for Online<'_> {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::Relaxed);
            if let Some(c) = self.1 {
                c.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }
    let on_server = proxy.counts.get(server_name);
    if let Some(c) = on_server {
        c.fetch_add(1, Ordering::Relaxed);
    }
    let _online = Online(&proxy.online, on_server);
    // Leaving the network (not moving between servers: that's a new connection first) keeps a line place for a while.
    struct Away<'a>(&'a Proxy, u128);
    impl Drop for Away<'_> {
        fn drop(&mut self) {
            if self.0.config.queue.enabled {
                let uuid = self.1;
                let p = self.0;
                let mut q = p.queue();
                // Another connection of theirs (the next server) may already be up: then they're still here.
                if !p.connected_elsewhere(uuid) {
                    q.left(uuid);
                }
            }
        }
    }
    proxy.sessions.lock().unwrap_or_else(|e| e.into_inner()).entry(uuid).and_modify(|n| *n += 1).or_insert(1);
    let _away = Away(proxy, uuid);
    struct Session<'a>(&'a Proxy, u128);
    impl Drop for Session<'_> {
        fn drop(&mut self) {
            let mut s = self.0.sessions.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(n) = s.get_mut(&self.1) {
                *n -= 1;
                if *n == 0 {
                    s.remove(&self.1);
                }
            }
        }
    }
    let _session = Session(proxy, uuid);
    let early_backend = backend.buffer().to_vec();
    let mut backend = backend.into_inner();
    let early_client = std::mem::take(&mut client.buf);
    if !early_client.is_empty() {
        send(&mut backend, &early_client).await?;
    }
    if !early_backend.is_empty() {
        client.send(&early_backend).await?;
    }
    let size = proxy.config.buffer.max(4096);
    match (client.dec.take(), client.enc.take()) {
        // Offline: a plain copy each way, nothing touched.
        (None, None) => {
            let mut stream = client.stream;
            let _ = tokio::io::copy_bidirectional_with_sizes(&mut stream, &mut backend, size, size).await;
        }
        // Online: the player's bytes decrypted on the way in, the server's encrypted on the way out.
        (Some(mut dec), Some(mut enc)) => {
            let (mut cr, mut cw) = client.stream.into_split();
            let (mut br, mut bw) = backend.into_split();
            let up = async move {
                let mut buf = vec![0u8; size];
                loop {
                    let n = match cr.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    dec.decrypt(&mut buf[..n]);
                    if bw.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
                let _ = bw.shutdown().await;
            };
            let down = async move {
                let mut buf = vec![0u8; size];
                loop {
                    let n = match br.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    enc.encrypt(&mut buf[..n]);
                    if cw.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
                let _ = cw.shutdown().await;
            };
            tokio::join!(up, down);
        }
        _ => unreachable!("both ciphers or neither"),
    }
    Ok(())
}

/// The control port: lines `<secret> SEND <player uuid or name> <server>` (the player's next login goes there,
/// within a minute) and `<secret> COUNTS` (JSON: players per server and in all). One answer line each.
async fn control(proxy: Arc<Proxy>) {
    let listener = match TcpListener::bind(&proxy.config.control).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("control port {}: {e}", proxy.config.control);
            return;
        }
    };
    println!("control port on {}", proxy.config.control);
    loop {
        let Ok((stream, _)) = listener.accept().await else { continue };
        let proxy = proxy.clone();
        tokio::spawn(async move {
            let (r, mut w) = stream.into_split();
            let mut lines = tokio::io::AsyncBufReadExt::lines(BufReader::new(r));
            while let Ok(Ok(Some(line))) = tokio::time::timeout(Duration::from_secs(10), lines.next_line()).await {
                let answer = control_line(&proxy, &line);
                if w.write_all(format!("{answer}\n").as_bytes()).await.is_err() {
                    break;
                }
            }
        });
    }
}

fn control_line(proxy: &Proxy, line: &str) -> String {
    let mut words = line.split_whitespace();
    if words.next().map(str::as_bytes) != Some(proxy.secret.as_slice()) {
        return "ERR secret".into();
    }
    match words.next().map(str::to_ascii_uppercase).as_deref() {
        Some("SEND") => {
            let (Some(player), Some(server)) = (words.next(), words.next()) else {
                return "ERR usage: SEND <player> <server>".into();
            };
            if !proxy.config.servers.contains_key(server) {
                return format!("ERR no server {server}");
            }
            // A queued server: straight in, a place in its line, or "the line is full".
            let hex = player.replace('-', "").to_ascii_lowercase();
            if let Ok(uuid) = u128::from_str_radix(&hex, 16)
                && hex.len() == 32
                && proxy.queue().is_queued(server)
            {
                let name = words.next().unwrap_or(player).to_owned();
                let tier = tier_of(proxy, uuid);
                let counts = proxy.count_map();
                let reply = proxy.queue().request(uuid, &name, tier, server, &counts);
                match reply {
                    queue::Reply::Go => {}
                    queue::Reply::Queued(st) => return format!("QUEUED {}", where_json(proxy, &st)),
                    queue::Reply::Full(n) => return format!("ERR queue-full {n}"),
                }
            }
            let key = player.replace('-', "").to_ascii_lowercase();
            proxy.pending.lock().unwrap_or_else(|e| e.into_inner()).insert(key, (server.to_owned(), std::time::Instant::now()));
            "OK".into()
        }
        Some("COUNTS") => {
            let servers: serde_json::Map<String, serde_json::Value> =
                proxy.counts.iter().map(|(k, v)| (k.clone(), serde_json::json!(v.load(Ordering::Relaxed)))).collect();
            serde_json::json!({ "total": proxy.online.load(Ordering::Relaxed), "servers": servers }).to_string()
        }
        Some("HEALTH") => {
            // HEALTH <server> <tps> <mspt> <memory percent>, from each server's plugin every few seconds.
            let (Some(server), Some(tps), Some(mspt), Some(mem)) = (words.next(), words.next(), words.next(), words.next()) else {
                return "ERR usage: HEALTH <server> <tps> <mspt> <memory %>".into();
            };
            let num = |s: &str| s.parse::<f64>().unwrap_or(0.0);
            proxy.queue().health.insert(server.to_owned(), queue::Health { tps: num(tps), mspt: num(mspt), memory_percent: num(mem), at: Some(std::time::Instant::now()) });
            "OK".into()
        }
        Some("QUEUE") => queue_command(proxy, words.collect()),
        _ => "ERR commands: SEND, COUNTS, HEALTH, QUEUE".into(),
    }
}

/// `QUEUE` (status as JSON), `QUEUE WHERE <uuid>...` (each one's place, or their turn), `QUEUE LEAVE <uuid>`,
/// `QUEUE PAUSE|RESUME`, `QUEUE NEXT <player>` (their turn now), `QUEUE REMOVE <player>`.
fn queue_command(proxy: &Proxy, args: Vec<&str>) -> String {
    if !proxy.config.queue.enabled {
        return "ERR the queue is off ([queue] enabled = false)".into();
    }
    let counts = proxy.count_map();
    let parse = |w: &str| -> Option<u128> {
        let hex = w.replace('-', "").to_ascii_lowercase();
        (hex.len() == 32).then(|| u128::from_str_radix(&hex, 16).ok()).flatten()
    };
    let find = |q: &queue::Queue, who: &str| -> Option<u128> {
        parse(who).filter(|u| q.entries.contains_key(u)).or_else(|| q.entries.iter().find(|(_, e)| e.name.eq_ignore_ascii_case(who)).map(|(u, _)| *u))
    };
    match args.first().map(|a| a.to_ascii_uppercase()).as_deref() {
        None | Some("STATUS") => {
            let q = proxy.queue();
            let now = std::time::Instant::now();
            let lines: serde_json::Map<String, serde_json::Value> = q
                .cfg
                .queued
                .iter()
                .map(|s| {
                    let by_tier: serde_json::Map<String, serde_json::Value> = q.cfg.tiers.iter().filter(|t| !t.bypass).map(|t| (t.name.clone(), serde_json::json!(q.entries.values().filter(|e| e.connected && e.server == *s && q.cfg.tiers.get(e.tier).is_some_and(|x| x.name == t.name)).count()))).collect();
                    let h = q.health.get(s);
                    (s.clone(), serde_json::json!({
                        "waiting": q.waiting_for(s),
                        "tiers": by_tier,
                        "on": counts.get(s).copied().unwrap_or(0),
                        "max": q.limits(s).max_players,
                        "per_minute": (q.rate_for(s, now) * 600.0).round() / 10.0,
                        "held": q.held(s, &counts),
                        "health": h.map(|h| serde_json::json!({ "tps": h.tps, "mspt": h.mspt, "memory": h.memory_percent, "age_s": h.at.map(|a| now.saturating_duration_since(a).as_secs()) })),
                    }))
                })
                .collect();
            serde_json::json!({
                "lines": lines,
                "away": q.entries.values().filter(|e| !e.connected).count(),
                "on_their_way": q.tickets.len(),
                "admitted": q.total_admitted,
                "startup": q.in_startup(now),
                "paused": q.paused,
            })
            .to_string()
        }
        Some("WHERE") => {
            // For the hub's plugin, once a second: each player it asks about who has a place or a ticket.
            let q = proxy.queue();
            let mut out = serde_json::Map::new();
            for w in &args[1..] {
                let Some(uuid) = parse(w) else { continue };
                if let Some(server) = q.ticket(uuid) {
                    out.insert(format!("{uuid:032x}"), serde_json::json!({ "go": server, "display": q.display(server) }));
                } else if let Some(st) = q.standing(uuid, &counts) {
                    out.insert(format!("{uuid:032x}"), standing_json(&q, &st));
                }
            }
            serde_json::Value::Object(out).to_string()
        }
        Some("LEAVE") => {
            let Some(uuid) = args.get(1).and_then(|w| parse(w)) else { return "ERR usage: QUEUE LEAVE <uuid>".into() };
            let mut q = proxy.queue();
            let had = q.entries.contains_key(&uuid);
            q.remove(uuid);
            if had { "OK".into() } else { "ERR not in a line".into() }
        }
        Some("PAUSE") => {
            proxy.queue().paused = true;
            "OK paused".into()
        }
        Some("RESUME") => {
            proxy.queue().paused = false;
            "OK resumed".into()
        }
        Some("NEXT") => {
            let Some(who) = args.get(1) else { return "ERR usage: QUEUE NEXT <player>".into() };
            let mut q = proxy.queue();
            let Some(uuid) = find(&q, who) else { return format!("ERR {who} isn't waiting") };
            let server = q.entries[&uuid].server.clone();
            let expires = queue::now_ms() + q.cfg.ticket_seconds * 1000;
            q.remove(uuid);
            q.tickets.insert(uuid, (server, expires));
            "OK".into()
        }
        Some("REMOVE") => {
            let Some(who) = args.get(1) else { return "ERR usage: QUEUE REMOVE <player>".into() };
            let mut q = proxy.queue();
            let Some(uuid) = find(&q, who) else { return format!("ERR {who} isn't waiting") };
            q.remove(uuid);
            "OK".into()
        }
        _ => "ERR usage: QUEUE [STATUS|WHERE <uuid>...|LEAVE <uuid>|PAUSE|RESUME|NEXT <player>|REMOVE <player>]".into(),
    }
}

/// A player's place as the hub's plugin shows it.
fn standing_json(q: &queue::Queue, st: &queue::Standing) -> serde_json::Value {
    let tier = q.cfg.tiers.get(st.tier);
    let default = st.tier == q.default_tier();
    serde_json::json!({
        "server": st.server,
        "display": q.display(&st.server),
        "position": st.position,
        "total": st.total,
        "eta": screen::eta_text(st.eta_seconds),
        "tier": if default { String::new() } else { tier.map(|t| if t.display.is_empty() { t.name.clone() } else { t.display.clone() }).unwrap_or_default() },
        "tier_color": tier.map(|t| t.color.clone()).unwrap_or_default(),
        "held": st.held,
    })
}

fn where_json(proxy: &Proxy, st: &queue::Standing) -> String {
    standing_json(&proxy.queue(), st).to_string()
}

// ---- The queue ----

fn tier_of(proxy: &Proxy, uuid: u128) -> usize {
    let q = &proxy.config.queue;
    let (groups, perms) = queue::lp_lookup(&q.luckperms, uuid);
    let default = proxy.queue().default_tier();
    queue::tier_for(q, &groups, &perms, default)
}

fn queue_sample(proxy: &Proxy) -> Vec<serde_json::Value> {
    if !proxy.config.queue.enabled {
        return Vec::new();
    }
    let q = proxy.queue();
    let lines: Vec<(String, usize)> = q.cfg.queued.iter().map(|s| (q.display(s), q.waiting_for(s))).collect();
    screen::status_sample(&lines)
}

/// Four times a second: whose turn it is (their ticket waits for the hub's plugin to move them); every few seconds
/// the lines are saved if they changed.
async fn admissions(proxy: Arc<Proxy>) {
    let mut every = tokio::time::interval(Duration::from_millis(250));
    let mut saved = std::time::Instant::now();
    let mut logged = std::time::Instant::now();
    loop {
        every.tick().await;
        let counts = proxy.count_map();
        let (admitted, save) = {
            let mut q = proxy.queue();
            let admitted = q.tick(&counts);
            let save = (q.dirty && saved.elapsed() > Duration::from_secs(2)).then(|| {
                q.dirty = false;
                q.save_json()
            });
            (admitted.len(), save)
        };
        if let Some(text) = save {
            saved = std::time::Instant::now();
            let path = proxy.queue_file.clone();
            tokio::task::spawn_blocking(move || {
                let tmp = path.with_extension("tmp");
                if std::fs::write(&tmp, text).is_ok() {
                    let _ = std::fs::rename(&tmp, &path);
                }
            });
        }
        if logged.elapsed() > Duration::from_secs(30) {
            let q = proxy.queue();
            if q.waiting() > 0 || admitted > 0 {
                println!("queue: {} waiting, {} admitted so far", q.waiting(), q.total_admitted);
            }
            logged = std::time::Instant::now();
        }
    }
}

/// A player with a place in a line waits here: the login finished by the proxy itself, then the configuration stage
/// (no world, no server: a black screen) with the queue screen. When their ticket comes, the game's Transfer sends
/// them back through the proxy to their server; "Leave Queue" gives up the place and sends them back to the hub.
async fn wait_in_queue(proxy: &Proxy, mut client: Client, intent: &handshake::Intention, uuid: u128, name: String, properties: Vec<login::Property>) -> Result<()> {
    let finished = login::Clientbound::LoginFinished(login::LoginFinished { profile: login::GameProfile { uuid, name: name.clone(), properties }, session: uuid }).encode();
    client.send(&frame_of(&finished, None)?).await?;
    loop {
        let (id, payload) = packet_of(client.read_frame().await?, None)?;
        match login::Serverbound::decode(id, &payload).map_err(|e| e.to_string())? {
            login::Serverbound::LoginAcknowledged(_) => break,
            login::Serverbound::CustomQueryAnswer(_) | login::Serverbound::CookieResponse(_) => {}
            _ => return Err("unexpected packet while logging in to the queue".into()),
        }
    }
    let came_back = proxy.queue().entries.get(&uuid).is_some_and(|e| e.left_at > 0 && queue::now_ms().saturating_sub(e.left_at) > 10_000);
    // The player's bytes are read on their own task (with the cipher), so the screen can be written meanwhile.
    let Client { stream, buf, dec, enc } = client;
    let (mut rd, mut wr) = stream.into_split();
    let (ftx, mut frx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let reader = tokio::spawn(async move {
        let mut buf = buf;
        let mut dec = dec;
        loop {
            match frame::split(&mut buf) {
                Ok(Some(f)) => {
                    if ftx.send(f).is_err() {
                        return;
                    }
                    continue;
                }
                Ok(None) => {}
                Err(_) => return,
            }
            let mut chunk = [0u8; 4096];
            let n = match rd.read(&mut chunk).await {
                Ok(0) | Err(_) => return,
                Ok(n) => n,
            };
            if let Some(d) = dec.as_mut() {
                d.decrypt(&mut chunk[..n]);
            }
            buf.extend_from_slice(&chunk[..n]);
        }
    });
    let mut enc = enc;
    let mut out = |body: Vec<u8>| -> Result<Vec<u8>> {
        let mut bytes = frame_of(&body, None)?;
        if let Some(e) = enc.as_mut() {
            e.encrypt(&mut bytes);
        }
        Ok(bytes)
    };
    let qc = &proxy.config.queue;
    // The queue world (2b2t style) when its recording is there and the game knows the core pack; else the screen.
    let mut in_world = false;
    if let Some(lim) = proxy.limbo.as_ref() {
        in_world = true;
        for r in &lim.records {
            send(&mut wr, &out(r.body.clone())?).await?;
            if r.known_packs {
                let packs = loop {
                    let Ok(Some(f)) = tokio::time::timeout(Duration::from_secs(15), frx.recv()).await else {
                        proxy.queue().left(uuid);
                        reader.abort();
                        return Ok(());
                    };
                    if let Ok((id, p)) = packet_of(f, None)
                        && let Ok(cfg::Serverbound::SelectKnownPacks(k)) = cfg::Serverbound::decode(id, &p)
                    {
                        break k.packs;
                    }
                };
                if !packs.iter().any(|p| p.namespace == "minecraft" && p.id == "core") {
                    in_world = false;
                    break;
                }
            }
        }
        if in_world {
            send(&mut wr, &out(cfg::Clientbound::FinishConfiguration(horizon_protocol::packets::Empty).encode())?).await?;
            loop {
                let Ok(Some(f)) = tokio::time::timeout(Duration::from_secs(15), frx.recv()).await else {
                    proxy.queue().left(uuid);
                    reader.abort();
                    return Ok(());
                };
                if let Ok((id, p)) = packet_of(f, None)
                    && let Ok(cfg::Serverbound::FinishConfiguration(_)) = cfg::Serverbound::decode(id, &p)
                {
                    break;
                }
            }
            for body in lim.join_packets() {
                send(&mut wr, &out(body)?).await?;
            }
        }
    }
    if !in_world && !qc.pack_url.is_empty() {
        let id = u128::from_str_radix(&qc.pack_id.replace('-', ""), 16).unwrap_or(0x8b2f_4c1d_9e6a_4f7b_8c3d_2e1f_0a9b_8c7d);
        let push = cfg::ResourcePackPush { id, url: qc.pack_url.clone(), hash: qc.pack_sha1.to_ascii_lowercase(), required: false, prompt: None };
        send(&mut wr, &out(cfg::Clientbound::ResourcePackPush(push).encode())?).await?;
    }
    let mut every = tokio::time::interval(Duration::from_secs_f64(qc.refresh_seconds.max(0.5)));
    let mut ticks = 0u64;
    let mut shown: Option<queue::Standing> = None;
    let mut told_position = None;
    // Where they go next: their server (their turn came), the hub (they left the line), or nowhere (gone).
    let next: Option<bool> = loop {
        tokio::select! {
            f = frx.recv() => match f {
                None => break None,
                Some(frame) => {
                    let Ok((id, payload)) = packet_of(frame, None) else { continue };
                    let leave = if in_world {
                        match play::Serverbound::decode(id, &payload) {
                            Ok(play::Serverbound::ChatCommand(c)) => limbo::is_leave(&c.command),
                            Ok(play::Serverbound::ChatCommandSigned(c)) => limbo::is_leave(&c.command),
                            _ => false,
                        }
                    } else {
                        matches!(cfg::Serverbound::decode(id, &payload), Ok(cfg::Serverbound::CustomClickAction(a)) if a.id.ends_with("queue/leave"))
                    };
                    if leave {
                        proxy.queue().remove(uuid);
                        break Some(false);
                    }
                }
            },
            _ = every.tick() => {
                ticks += 1;
                let counts = proxy.count_map();
                let (ticket, standing, display) = {
                    let q = proxy.queue();
                    let st = q.standing(uuid, &counts);
                    let display = st.as_ref().map(|s| q.display(&s.server)).unwrap_or_default();
                    (q.ticket(uuid).is_some(), st, display)
                };
                if ticket {
                    break Some(true);
                }
                let Some(s) = standing else {
                    // Taken out of the line (by the staff, or the place ran out): back to the hub.
                    break Some(false);
                };
                if in_world {
                    let default_tier = qc.tiers.iter().rposition(|t| t.groups.is_empty() && t.permission.is_empty() && !t.bypass);
                    let tier = qc.tiers.get(s.tier).filter(|_| Some(s.tier) != default_tier).map(|t| {
                        (if t.display.is_empty() { t.name.as_str() } else { t.display.as_str() }, if t.color.is_empty() { "#fbbf24" } else { t.color.as_str() })
                    });
                    // A staff pause is not shown (owner); a busy server is.
                    let held = s.held.as_deref().filter(|w| !w.starts_with("paused")).map(|w| {
                        let mut c = w.chars();
                        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
                    });
                    for body in limbo::screen_packets(s.position, &screen::eta_text(s.eta_seconds), tier, held.as_deref()) {
                        send(&mut wr, &out(body)?).await?;
                    }
                    // In chat: the place when it changed (at most every 20 s), the store every minute.
                    if told_position != Some(s.position) && (ticks == 1 || ticks % 10 == 0) {
                        send(&mut wr, &out(limbo::chat_position(s.position))?).await?;
                        told_position = Some(s.position);
                    }
                    if (ticks == 1 || ticks % 30 == 0)
                        && tier.is_none()
                        && let Some(body) = limbo::chat_store(&qc.store_url)
                    {
                        send(&mut wr, &out(body)?).await?;
                    }
                    if ticks % 5 == 0 {
                        send(&mut wr, &out(play::Clientbound::KeepAlive(cfg::KeepAlive { id: ticks as i64 }).encode())?).await?;
                    }
                    shown = Some(s);
                    continue;
                }
                if shown.as_ref() != Some(&s) || ticks % 5 == 0 {
                    let dialog = screen::queue_dialog(qc, &display, &s, came_back && ticks < 4);
                    send(&mut wr, &out(cfg::Clientbound::ShowDialog(cfg::ShowDialog { dialog }).encode())?).await?;
                    shown = Some(s);
                }
                if ticks % 5 == 0 {
                    send(&mut wr, &out(cfg::Clientbound::KeepAlive(cfg::KeepAlive { id: ticks as i64 }).encode())?).await?;
                }
            }
        }
    };
    match next {
        // Their turn (the ticket routes the next login) or back to the hub (no place, no ticket: the usual way in).
        Some(_) => {
            let host = if qc.transfer_host.is_empty() { intent.host.split('\0').next().unwrap_or(&intent.host).trim_end_matches('.').to_owned() } else { qc.transfer_host.clone() };
            let port = if qc.transfer_port == 0 { intent.port } else { qc.transfer_port };
            if in_world {
                send(&mut wr, &out(play::Clientbound::ClearTitles(play::Bool1 { value: true }).encode())?).await?;
                send(&mut wr, &out(play::Clientbound::Transfer(cfg::Transfer { host, port: i32::from(port) }).encode())?).await?;
            } else {
                send(&mut wr, &out(cfg::Clientbound::ClearDialog(horizon_protocol::packets::Empty).encode())?).await?;
                send(&mut wr, &out(cfg::Clientbound::Transfer(cfg::Transfer { host, port: i32::from(port) }).encode())?).await?;
            }
            // The game closes this connection itself as it follows the transfer.
            let _ = tokio::time::timeout(Duration::from_secs(5), async { while frx.recv().await.is_some() {} }).await;
        }
        None => proxy.queue().left(uuid),
    }
    reader.abort();
    Ok(())
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The proxy's CFB8 (the CPU's AES) gives the same bytes as the protocol crate's (the client's), both ways,
    /// across reads of any size.
    #[test]
    fn cfb8_matches_the_protocol_crate() {
        let key: [u8; 16] = std::array::from_fn(|i| (i as u8).wrapping_mul(37).wrapping_add(11));
        let plain: Vec<u8> = (0..5000u32).map(|i| (i * 7 + i / 13) as u8).collect();
        let mut theirs = plain.clone();
        horizon_protocol::crypto::Cfb8::new(&key, &key).encrypt(&mut theirs);
        let mut ours = plain.clone();
        let mut enc = Cfb8::new(&key);
        for part in ours.chunks_mut(333) {
            enc.encrypt(part);
        }
        assert_eq!(ours, theirs, "encryption");
        let mut dec = Cfb8::new(&key);
        for part in ours.chunks_mut(17) {
            dec.decrypt(part);
        }
        assert_eq!(ours, plain, "decryption");
    }

    #[test]
    fn base64_matches() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }

    /// The forwarding data the servers check: HMAC-SHA256 over version 1, address, UUID, name and properties.
    #[test]
    fn forwarding_payload_is_signed_and_laid_out() {
        use hmac::{Hmac, Mac};
        let address: SocketAddr = "203.0.113.5:4000".parse().unwrap();
        let props = vec![login::Property { name: "textures".into(), value: "abc".into(), signature: Some("sig".into()) }];
        let out = forwarding_payload(b"secret", &address, 0x1234, "Steve", &props);
        let (sig, data) = out.split_at(32);
        let mut mac = <Hmac<sha2::Sha256> as Mac>::new_from_slice(b"secret").unwrap();
        mac.update(data);
        assert_eq!(sig, mac.finalize().into_bytes().as_slice());
        let mut r = Reader::new(data);
        assert_eq!(r.varint().unwrap(), 1);
    }
}
