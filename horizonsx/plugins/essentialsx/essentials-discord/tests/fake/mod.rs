//! A small Discord on 127.0.0.1: REST and the gateway, as far as the module uses them, and a `Host` with
//! no game behind it. Nothing here goes near the real service.
use essentials_discord::client::ClientOptions;
use essentials_discord::host::{Feedback, Host, Level, Relay, Task};
use essentials_discord::service::JdaDiscordService;
use essentials_discord::settings::DiscordSettings;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tungstenite::Message as Frame;
use tungstenite::protocol::CloseFrame;
use tungstenite::protocol::frame::coding::CloseCode;

pub const TOKEN: &str = "fake.bot-token.do-not-log";
pub const GUILD: u64 = 111_111_111_111_111_111;
pub const PRIMARY: u64 = 222_222_222_222_222_222;
pub const STAFF: u64 = 333_333_333_333_333_333;
pub const CONSOLE: u64 = 444_444_444_444_444_444;
pub const BOT_ROLE: u64 = 555_555_555_555_555_555;
pub const ADMIN_ROLE: u64 = 666_666_666_666_666_666;
pub const COLOR_ROLE: u64 = 777_777_777_777_777_777;
pub const SELF_ID: u64 = 900_000_000_000_000_001;
pub const APP_ID: u64 = 900_000_000_000_000_002;
pub const USER_ID: u64 = 900_000_000_000_000_003;

#[derive(Clone, Debug)]
pub struct Recorded {
    pub method: String,
    pub path: String,
    pub auth: Option<String>,
    pub body: Value,
    pub at: Instant,
}

/// A scripted answer: the next request that matches gets it, once.
struct Canned {
    method: String,
    path_contains: String,
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

pub enum Command {
    Send(Value),
    /// The socket goes, with no close frame.
    Drop,
    Close(u16),
}

#[derive(Default)]
struct State {
    requests: Mutex<Vec<Recorded>>,
    canned: Mutex<VecDeque<Canned>>,
    frames: Mutex<Vec<Value>>,
    commands: Mutex<VecDeque<Command>>,
    connections: AtomicU64,
    /// The next IDENTIFY gets this close code.
    close_on_identify: Mutex<Option<u16>>,
    /// Refuse RESUME with an invalid session.
    refuse_resume: AtomicBool,
    /// Stop acknowledging heartbeats.
    silent: AtomicBool,
    heartbeat_ms: AtomicU64,
    closes_seen: Mutex<Vec<u16>>,
    seq: AtomicU64,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct FakeDiscord {
    state: Arc<State>,
    pub rest_port: u16,
    pub gateway_port: u16,
}

fn guild_json() -> Value {
    let perms = |bits: u64| bits.to_string();
    json!({
        "id": GUILD.to_string(),
        "name": "Fake Guild",
        "owner_id": "1",
        "roles": [
            { "id": GUILD.to_string(), "name": "@everyone", "position": 0, "permissions": perms((1 << 10) | (1 << 11)) },
            { "id": BOT_ROLE.to_string(), "name": "Bot", "position": 5, "permissions": perms((1 << 29) | (1 << 28) | (1 << 27) | (1 << 14) | (1 << 10) | (1 << 11)) },
            { "id": ADMIN_ROLE.to_string(), "name": "Admins", "position": 4, "permissions": perms(1 << 3) },
            { "id": COLOR_ROLE.to_string(), "name": "Coloured", "position": 3, "permissions": "0", "color": 0xff0000 },
        ],
        "channels": [
            { "id": PRIMARY.to_string(), "name": "general", "type": 0, "position": 0 },
            { "id": STAFF.to_string(), "name": "staff", "type": 0, "position": 1 },
            { "id": CONSOLE.to_string(), "name": "console", "type": 0, "position": 2 },
        ],
        "emojis": [{ "id": "4242", "name": "party", "animated": false }],
        "members": [{ "user": { "id": SELF_ID.to_string(), "username": "EssBot", "discriminator": "0", "bot": true }, "nick": "Essentials", "roles": [BOT_ROLE.to_string()] }],
    })
}

fn respond(stream: &mut TcpStream, status: u16, headers: &[(String, String)], body: &str) {
    let extra: String = headers.iter().map(|(name, value)| format!("{name}: {value}\r\n")).collect();
    let answer = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}", body.len());
    let _ = stream.write_all(answer.as_bytes());
}

fn default_answer(method: &str, path: &str, body: &Value, gateway_port: u16) -> (u16, Value) {
    let plain = path.split('?').next().unwrap_or(path);
    match (method, plain) {
        ("GET", "/users/@me") => (200, json!({ "id": SELF_ID.to_string(), "username": "EssBot", "discriminator": "0", "bot": true })),
        ("GET", "/gateway/bot") => (200, json!({ "url": format!("ws://127.0.0.1:{gateway_port}"), "session_start_limit": { "total": 1000, "remaining": 999, "reset_after": 0, "max_concurrency": 1 } })),
        ("GET", path) if path.ends_with("/emojis") => (200, json!([{ "id": "4242", "name": "party" }])),
        ("PUT", path) if path.contains("/commands") => (200, Value::Array(body.as_array().into_iter().flatten().enumerate().map(|(index, row)| json!({ "id": (index + 1).to_string(), "name": row["name"] })).collect())),
        ("POST", path) if path.contains("/commands") => (200, json!({ "id": "9", "name": body["name"] })),
        ("POST", path) if path.ends_with("/messages") => (200, json!({ "id": "1" })),
        ("GET", path) if path.ends_with("/webhooks") => (200, json!([])),
        ("POST", path) if path.ends_with("/webhooks") => (200, json!({ "id": "8800", "name": body["name"], "token": "fake-hook-token" })),
        ("POST", path) if path.starts_with("/webhooks/") => (204, Value::Null),
        ("POST", path) if path.starts_with("/interactions/") => (204, Value::Null),
        ("PATCH", path) if path.contains("@original") => (200, json!({})),
        ("POST", "/users/@me/channels") => (200, json!({ "id": "7700" })),
        ("GET", path) if path.contains("/members/") => {
            if path.ends_with(&USER_ID.to_string()) {
                (200, json!({ "user": { "id": USER_ID.to_string(), "username": "someone", "discriminator": "0" }, "roles": [COLOR_ROLE.to_string()] }))
            } else {
                (404, json!({ "code": 10007, "message": "Unknown Member" }))
            }
        }
        ("PATCH", path) if path.contains("/members/") => (200, json!({})),
        _ => (404, json!({ "code": 0, "message": "the fake doesn't know this" })),
    }
}

fn serve_rest(mut stream: TcpStream, state: &State, gateway_port: u16) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut bytes = Vec::new();
    let mut piece = [0u8; 4096];
    let (head_end, length) = loop {
        let Ok(read) = stream.read(&mut piece) else {
            return;
        };
        if read == 0 {
            return;
        }
        bytes.extend_from_slice(&piece[..read]);
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
            let length = head.lines().find_map(|line| line.strip_prefix("content-length:")).and_then(|text| text.trim().parse::<usize>().ok()).unwrap_or(0);
            break (end + 4, length);
        }
    };
    while bytes.len() < head_end + length {
        let Ok(read) = stream.read(&mut piece) else {
            return;
        };
        if read == 0 {
            return;
        }
        bytes.extend_from_slice(&piece[..read]);
    }
    let head = String::from_utf8_lossy(&bytes[..head_end]).into_owned();
    let mut words = head.split_whitespace();
    let (method, path) = (words.next().unwrap_or_default().to_owned(), words.next().unwrap_or_default().to_owned());
    let auth = head.lines().find_map(|line| line.split_once(':').filter(|(name, _)| name.eq_ignore_ascii_case("authorization")).map(|(_, value)| value.trim().to_owned()));
    let body: Value = serde_json::from_slice(&bytes[head_end..head_end + length]).unwrap_or(Value::Null);
    locked(&state.requests).push(Recorded { method: method.clone(), path: path.clone(), auth, body: body.clone(), at: Instant::now() });
    let canned = {
        let mut canned = locked(&state.canned);
        canned.iter().position(|row| row.method == method && path.contains(&row.path_contains)).and_then(|index| canned.remove(index))
    };
    match canned {
        Some(row) => respond(&mut stream, row.status, &row.headers, &row.body),
        None => {
            let (status, answer) = default_answer(&method, &path, &body, gateway_port);
            respond(&mut stream, status, &[], &if answer.is_null() { String::new() } else { answer.to_string() });
        }
    }
}

type Socket = tungstenite::WebSocket<TcpStream>;

fn send(socket: &mut Socket, value: Value) -> bool {
    socket.send(Frame::text(value.to_string())).is_ok()
}

fn dispatch(socket: &mut Socket, state: &State, kind: &str, data: Value) -> bool {
    let seq = state.seq.fetch_add(1, Ordering::SeqCst) + 1;
    send(socket, json!({ "op": 0, "t": kind, "s": seq, "d": data }))
}

fn serve_gateway(stream: TcpStream, state: &State) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let Ok(mut socket) = tungstenite::accept(stream) else {
        return;
    };
    let _ = socket.get_ref().set_read_timeout(Some(Duration::from_millis(10)));
    state.connections.fetch_add(1, Ordering::SeqCst);
    send(&mut socket, json!({ "op": 10, "d": { "heartbeat_interval": state.heartbeat_ms.load(Ordering::SeqCst) } }));
    loop {
        let command = locked(&state.commands).pop_front();
        match command {
            Some(Command::Send(value)) => {
                dispatch(&mut socket, state, value["t"].as_str().unwrap_or_default(), value["d"].clone());
            }
            Some(Command::Drop) => {
                let _ = socket.get_ref().shutdown(std::net::Shutdown::Both);
                return;
            }
            Some(Command::Close(code)) => {
                let _ = socket.close(Some(CloseFrame { code: CloseCode::from(code), reason: "".into() }));
                let _ = socket.flush();
                return;
            }
            None => {}
        }
        match socket.read() {
            Ok(Frame::Text(text)) => {
                let value: Value = serde_json::from_str(text.as_str()).unwrap_or(Value::Null);
                locked(&state.frames).push(value.clone());
                match value["op"].as_u64() {
                    Some(1) if !state.silent.load(Ordering::SeqCst) => {
                        send(&mut socket, json!({ "op": 11 }));
                    }
                    Some(2) => {
                        if let Some(code) = locked(&state.close_on_identify).take() {
                            let _ = socket.close(Some(CloseFrame { code: CloseCode::from(code), reason: "".into() }));
                            let _ = socket.flush();
                            return;
                        }
                        let ready = json!({
                            "v": 10,
                            "session_id": "fake-session",
                            "resume_gateway_url": format!("ws://{}", socket.get_ref().local_addr().map(|address| address.to_string()).unwrap_or_default()),
                            "user": { "id": SELF_ID.to_string(), "username": "EssBot", "discriminator": "0", "bot": true, "avatar": "abc" },
                            "application": { "id": APP_ID.to_string() },
                            "guilds": [{ "id": GUILD.to_string(), "unavailable": true }],
                        });
                        dispatch(&mut socket, state, "READY", ready);
                        dispatch(&mut socket, state, "GUILD_CREATE", guild_json());
                    }
                    Some(6) => {
                        if state.refuse_resume.swap(false, Ordering::SeqCst) {
                            send(&mut socket, json!({ "op": 9, "d": false }));
                        } else {
                            dispatch(&mut socket, state, "RESUMED", json!({}));
                        }
                    }
                    _ => {}
                }
            }
            Ok(Frame::Close(frame)) => {
                locked(&state.closes_seen).push(frame.map_or(1005, |frame| frame.code.into()));
                let _ = socket.flush();
                return;
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(error)) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => return,
        }
    }
}

impl FakeDiscord {
    pub fn start() -> Self {
        let state = Arc::new(State::default());
        state.heartbeat_ms.store(60_000, Ordering::SeqCst);
        let rest = TcpListener::bind("127.0.0.1:0").unwrap();
        let gateway = TcpListener::bind("127.0.0.1:0").unwrap();
        let rest_port = rest.local_addr().unwrap().port();
        let gateway_port = gateway.local_addr().unwrap().port();
        let on_rest = state.clone();
        std::thread::spawn(move || {
            for stream in rest.incoming().flatten() {
                let state = on_rest.clone();
                std::thread::spawn(move || serve_rest(stream, &state, gateway_port));
            }
        });
        let on_gateway = state.clone();
        std::thread::spawn(move || {
            for stream in gateway.incoming().flatten() {
                let state = on_gateway.clone();
                std::thread::spawn(move || serve_gateway(stream, &state));
            }
        });
        Self { state, rest_port, gateway_port }
    }
    pub fn requests(&self) -> Vec<Recorded> {
        locked(&self.state.requests).clone()
    }
    pub fn requests_to(&self, method: &str, path_contains: &str) -> Vec<Recorded> {
        self.requests().into_iter().filter(|row| row.method == method && row.path.contains(path_contains)).collect()
    }
    pub fn frames(&self) -> Vec<Value> {
        locked(&self.state.frames).clone()
    }
    pub fn frames_op(&self, op: u64) -> Vec<Value> {
        self.frames().into_iter().filter(|frame| frame["op"].as_u64() == Some(op)).collect()
    }
    pub fn connections(&self) -> u64 {
        self.state.connections.load(Ordering::SeqCst)
    }
    pub fn closes_seen(&self) -> Vec<u16> {
        locked(&self.state.closes_seen).clone()
    }
    pub fn command(&self, command: Command) {
        locked(&self.state.commands).push_back(command);
    }
    /// A dispatch event from the gateway.
    pub fn event(&self, kind: &str, data: Value) {
        self.command(Command::Send(json!({ "t": kind, "d": data })));
    }
    pub fn set_heartbeat(&self, ms: u64) {
        self.state.heartbeat_ms.store(ms, Ordering::SeqCst);
    }
    pub fn stop_acking(&self) {
        self.state.silent.store(true, Ordering::SeqCst);
    }
    pub fn refuse_next_resume(&self) {
        self.state.refuse_resume.store(true, Ordering::SeqCst);
    }
    pub fn close_on_identify(&self, code: u16) {
        *locked(&self.state.close_on_identify) = Some(code);
    }
    pub fn answer_once(&self, method: &str, path_contains: &str, status: u16, headers: &[(&str, &str)], body: &str) {
        let headers = headers.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect();
        locked(&self.state.canned).push_back(Canned { method: method.to_owned(), path_contains: path_contains.to_owned(), status, headers, body: body.to_owned() });
    }
}

/// A `Host` for a test: `pump` is its main thread.
#[derive(Default)]
pub struct TestHost {
    main: Mutex<VecDeque<Task>>,
    pub logs: Mutex<Vec<(Level, String)>>,
    pub delivered: Mutex<Vec<Relay>>,
    pub commands: Mutex<Vec<String>>,
    pub debug: AtomicBool,
}

impl TestHost {
    pub fn pump(&self) {
        loop {
            let task = locked(&self.main).pop_front();
            match task {
                Some(task) => task(),
                None => return,
            }
        }
    }
    pub fn log_text(&self) -> String {
        locked(&self.logs).iter().map(|(_, text)| text.clone()).collect::<Vec<_>>().join("\n")
    }
}

impl Host for TestHost {
    fn run_main(&self, task: Task) {
        locked(&self.main).push_back(task);
    }
    fn is_main(&self) -> bool {
        false
    }
    fn write_log(&self, level: Level, text: &str) {
        locked(&self.logs).push((level, text.to_owned()));
    }
    fn is_debug(&self) -> bool {
        self.debug.load(Ordering::SeqCst)
    }
    fn deliver(&self, relay: Relay) {
        locked(&self.delivered).push(relay);
    }
    fn dispatch_command(&self, line: &str, feedback: Feedback) {
        locked(&self.commands).push(line.to_owned());
        feedback(format!("ran {line}"));
    }
}

/// Waits for something that happens on another thread.
pub fn eventually(what: &str, mut check: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while Instant::now() < until {
        if check() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("never happened: {what}");
}

static NEXT_FOLDER: AtomicU64 = AtomicU64::new(0);

pub fn base_config() -> String {
    format!(
        "token: \"{TOKEN}\"\nguild: {GUILD}\nchannels:\n  primary: {PRIMARY}\n  staff: {STAFF}\n  console-room: {CONSOLE}\n  zeros: 000000000000000000\nconsole:\n  channel: 000000000000000000\nmessage-types:\n  chat: primary\n  join: primary\n  kick: staff\n  server-start: primary\n  server-stop: primary\n  private-chat: none\nchat:\n  discord-filter: \"^[!?]\"\npermit-formatting-roles:\n  - Admins\ncommands:\n  execute:\n    allowed-roles:\n      - Admins\n  list:\n    allowed-roles:\n      - \"*\"\n"
    )
}

/// A service on a config, pointed at `fake`, not started.
pub fn service_on(fake: &FakeDiscord, host: &Arc<TestHost>, config: &str) -> Arc<JdaDiscordService> {
    let folder = std::env::temp_dir().join(format!("essx-discord-{}-{}", std::process::id(), NEXT_FOLDER.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join("config.yml");
    std::fs::write(&file, config).unwrap();
    let dyn_host: Arc<dyn Host> = host.clone();
    let settings = Arc::new(DiscordSettings::new(file, essentials_discord::CONFIG_TEMPLATE, dyn_host.clone()));
    settings.reload_config();
    let service = JdaDiscordService::new(settings, dyn_host);
    let port = fake.rest_port;
    service.set_client_options(move |options: &mut ClientOptions| {
        options.api_base = format!("http://127.0.0.1:{port}");
        options.reconnect_base = Duration::from_millis(10);
        options.identify_gap = Duration::ZERO;
        options.login_timeout = Duration::from_secs(10);
        options.guild_timeout = Duration::from_secs(2);
    });
    service
}

/// A started service, on a fresh fake.
pub fn started(config: &str) -> (FakeDiscord, Arc<TestHost>, Arc<JdaDiscordService>) {
    let fake = FakeDiscord::start();
    let host = Arc::new(TestHost::default());
    let service = service_on(&fake, &host, config);
    service.startup().unwrap();
    (fake, host, service)
}

/// A MESSAGE_CREATE, from `USER_ID` unless changed after.
pub fn message_event(channel: u64, content: &str) -> Value {
    json!({
        "id": "5000",
        "channel_id": channel.to_string(),
        "guild_id": GUILD.to_string(),
        "author": { "id": USER_ID.to_string(), "username": "someone", "discriminator": "0" },
        "member": { "roles": [COLOR_ROLE.to_string()], "nick": null },
        "content": content,
        "attachments": [],
        "mentions": [],
        "mention_roles": [],
    })
}
