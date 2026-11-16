//! The gateway, on one thread of its own: HELLO, IDENTIFY or RESUME, the heartbeat, and reconnecting
//! with a growing delay, except on the close codes that say another try can't work.
use essentials::prelude::*;
use super::model::{Guild, Interaction, Member, Message, Role, snowflake};
use super::{Inner, Readiness};
use crate::host::{self, Level};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tungstenite::client::IntoClientRequest;
use tungstenite::protocol::CloseFrame;
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Connector, Message as Frame, WebSocket};

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The most a read blocks, and so how long a heartbeat, a presence or a stop can wait.
const POLL: Duration = Duration::from_millis(100);
const MAX_BACKOFF: Duration = Duration::from_secs(900);

const OP_DISPATCH: u64 = 0;
const OP_HEARTBEAT: u64 = 1;
const OP_IDENTIFY: u64 = 2;
const OP_RESUME: u64 = 6;
const OP_RECONNECT: u64 = 7;
const OP_INVALID_SESSION: u64 = 9;
const OP_HELLO: u64 = 10;
const OP_HEARTBEAT_ACK: u64 = 11;

#[derive(Debug, PartialEq, Eq)]
pub enum CloseAction {
    /// Another try can't work: stop, and say why.
    Fatal(&'static str),
    /// The session is gone: IDENTIFY again.
    Fresh,
    Resume,
}

/// What a close code means.
pub fn classify(code: u16) -> CloseAction {
    match code {
        4004 => CloseAction::Fatal("The provided token is invalid!"),
        4010 => CloseAction::Fatal("Invalid shard"),
        4011 => CloseAction::Fatal("Sharding required: this bot is in too many guilds"),
        4012 => CloseAction::Fatal("Invalid gateway version"),
        4013 => CloseAction::Fatal("Invalid intents"),
        4014 => CloseAction::Fatal("Disallowed intents: turn on the Message Content intent for this bot in the Discord developer portal"),
        1000 | 1001 | 4007 | 4009 => CloseAction::Fresh,
        _ => CloseAction::Resume,
    }
}

/// `base`, doubling with each failure up to `MAX_BACKOFF`.
pub fn backoff(base: Duration, failures: u32) -> Duration {
    base.saturating_mul(1 << failures.min(16)).min(MAX_BACKOFF)
}

#[derive(Default)]
struct Session {
    id: Option<String>,
    seq: Option<u64>,
    resume_url: Option<String>,
}

enum Exit {
    Stop,
    Fatal(&'static str),
    /// Reconnect: with what's left of the session, if any.
    Reconnect,
}

fn wants_tls(url: &str) -> bool {
    url.starts_with("wss:")
}

fn host_and_port(url: &str) -> Option<(String, u16)> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?']).next()?;
    let port = if wants_tls(url) { 443 } else { 80 };
    match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() => Some((host.to_owned(), port.parse().ok()?)),
        _ => Some((authority.to_owned(), port)),
    }
}

/// `ProxySettings.setServer`: the proxy's address out of its URL.
fn proxy_address(proxy: &str) -> Option<(String, u16)> {
    let url = if proxy.contains("://") { proxy.to_owned() } else { format!("http://{proxy}") };
    host_and_port(&url)
}

fn open_tcp(host: &str, port: u16) -> io::Result<TcpStream> {
    let mut last = io::Error::new(ErrorKind::NotFound, "no address");
    for address in (host, port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
            Ok(stream) => return Ok(stream),
            Err(error) => last = error,
        }
    }
    Err(last)
}

/// HTTP `CONNECT` through `proxy`, leaving the stream at the far end.
fn open_through(proxy: &str, host: &str, port: u16) -> io::Result<TcpStream> {
    let (proxy_host, proxy_port) = proxy_address(proxy).ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "bad http-proxy-server"))?;
    let mut stream = open_tcp(&proxy_host, proxy_port)?;
    stream.set_read_timeout(Some(CONNECT_TIMEOUT))?;
    stream.write_all(format!("CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n\r\n").as_bytes())?;
    let mut answer = Vec::new();
    let mut byte = [0u8; 1];
    while !answer.ends_with(b"\r\n\r\n") {
        fail_if!(answer.len() > 8192 || stream.read(&mut byte)? == 0, io::Error::new(ErrorKind::ConnectionAborted, "the proxy closed the connection"));
        answer.push(byte[0]);
    }
    let status = String::from_utf8_lossy(&answer);
    fail_if!(status.split_whitespace().nth(1) != Some("200"), io::Error::new(ErrorKind::PermissionDenied, "the proxy refused the connection"));
    Ok(stream)
}

/// A TLS setup that names its crypto provider, so nothing depends on which crate features are on.
pub(crate) fn tls_connector(extra_roots: &[Vec<u8>]) -> Connector {
    let mut roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    for root in extra_roots {
        let _ = roots.add(rustls::pki_types::CertificateDer::from(root.clone()));
    }
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("ring's protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    Connector::Rustls(Arc::new(config))
}

fn underlying(socket: &Socket) -> Option<&TcpStream> {
    match socket.get_ref() {
        MaybeTlsStream::Plain(stream) => Some(stream),
        MaybeTlsStream::Rustls(stream) => Some(&stream.sock),
        _ => None,
    }
}

fn connect(url: &str, proxy: Option<&str>, extra_roots: &[Vec<u8>]) -> Result<Socket, String> {
    let (host, port) = host_and_port(url).ok_or("a bad gateway address")?;
    let stream = match proxy {
        Some(proxy) => open_through(proxy, &host, port),
        None => open_tcp(&host, port),
    }
    .map_err(|error| format!("{:?}", error.kind()))?;
    stream.set_nodelay(true).map_err(|error| error.to_string())?;
    stream.set_read_timeout(Some(CONNECT_TIMEOUT)).map_err(|error| error.to_string())?;
    stream.set_write_timeout(Some(CONNECT_TIMEOUT)).map_err(|error| error.to_string())?;
    let request = url.into_client_request().map_err(|error| error.to_string())?;
    let connector = wants_tls(url).then(|| tls_connector(extra_roots));
    let (socket, _) = tungstenite::client_tls_with_config(request, stream, None, connector).map_err(|error| match error {
        tungstenite::HandshakeError::Failure(error) => error.to_string(),
        tungstenite::HandshakeError::Interrupted(_) => "the handshake timed out".to_owned(),
    })?;
    underlying(&socket).ok_or("an unknown stream")?.set_read_timeout(Some(POLL)).map_err(|error| error.to_string())?;
    Ok(socket)
}

fn jitter() -> f64 {
    f64::from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |time| time.subsec_nanos())) / 1e9
}

struct Connection<'a> {
    inner: &'a Arc<Inner>,
    socket: Socket,
    session: &'a mut Session,
    next_beat: Option<Instant>,
    interval: Duration,
    awaiting_ack: bool,
    last_identify: &'a mut Option<Instant>,
    /// Guilds READY named that haven't come yet, and since when.
    pending_guilds: &'a mut HashSet<u64>,
    ready_since: &'a mut Option<Instant>,
    failures: &'a mut u32,
}

impl Connection<'_> {
    fn send(&mut self, value: &Value) -> Result<(), tungstenite::Error> {
        match self.socket.send(Frame::text(value.to_string())) {
            Err(tungstenite::Error::Io(error)) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => Ok(()),
            other => other,
        }
    }
    fn heartbeat(&mut self) -> Result<(), tungstenite::Error> {
        self.send(&json!({ "op": OP_HEARTBEAT, "d": self.session.seq }))
    }
    fn identify(&mut self) -> Result<(), tungstenite::Error> {
        if let Some(gap) = self.last_identify.map(|last| last.elapsed()).and_then(|since| self.inner.options.identify_gap.checked_sub(since)) {
            self.inner.pause(gap);
        }
        *self.last_identify = Some(Instant::now());
        let token = self.inner.options.token.clone();
        let presence = self.inner.presence();
        self.send(&json!({ "op": OP_IDENTIFY, "d": {
            "token": token.expose(),
            "intents": self.inner.options.intents,
            "compress": false,
            "large_threshold": 250,
            "properties": { "os": std::env::consts::OS, "browser": "EssentialsX", "device": "EssentialsX" },
            "presence": presence,
        } }))
    }
    fn resume(&mut self, id: String, seq: Option<u64>) -> Result<(), tungstenite::Error> {
        let token = self.inner.options.token.clone();
        self.send(&json!({ "op": OP_RESUME, "d": { "token": token.expose(), "session_id": id, "seq": seq } }))
    }
    fn on_ready(&mut self, data: &Value) {
        self.session.id = data["session_id"].as_str().map(str::to_owned);
        self.session.resume_url = data["resume_gateway_url"].as_str().map(str::to_owned);
        *self.pending_guilds = data["guilds"].as_array().into_iter().flatten().filter_map(|row| snowflake(&row["id"])).collect();
        *self.ready_since = Some(Instant::now());
        *self.failures = 0;
        self.inner.cache_mut().apply_ready(data);
        self.check_ready();
    }
    fn check_ready(&mut self) {
        if self.pending_guilds.is_empty() || self.ready_since.is_some_and(|since| since.elapsed() >= self.inner.options.guild_timeout) {
            self.inner.set_readiness(Readiness::Ready);
        }
    }
    fn dispatch(&mut self, kind: &str, data: &Value) {
        match kind {
            "READY" => self.on_ready(data),
            "RESUMED" => *self.failures = 0,
            "GUILD_CREATE" => {
                self.inner.cache_mut().apply(kind, data);
                if let Some(id) = snowflake(&data["id"]) {
                    self.pending_guilds.remove(&id);
                }
                self.check_ready();
            }
            "MESSAGE_CREATE" => {
                if let Some(message) = Message::from_json(data) {
                    self.inner.sink().on_message(&message);
                }
            }
            "INTERACTION_CREATE" => {
                if let Some(interaction) = Interaction::from_json(data) {
                    self.inner.sink().on_interaction(interaction);
                }
            }
            _ => self.inner.cache_mut().apply(kind, data),
        }
    }
    /// One frame from the gateway. `Some` ends the connection.
    fn on_frame(&mut self, text: &str) -> Result<Option<Exit>, tungstenite::Error> {
        let Ok(value) = serde_json::from_str::<Value>(text) else { return Ok(None) };
        match value["op"].as_u64() {
            Some(OP_DISPATCH) => {
                if let Some(seq) = value["s"].as_u64() {
                    self.session.seq = Some(seq);
                }
                self.dispatch(value["t"].as_str().unwrap_or_default(), &value["d"]);
            }
            Some(OP_HEARTBEAT) => self.heartbeat()?,
            Some(OP_HEARTBEAT_ACK) => self.awaiting_ack = false,
            Some(OP_RECONNECT) => return Ok(Some(Exit::Reconnect)),
            Some(OP_INVALID_SESSION) => {
                if value["d"].as_bool() != Some(true) {
                    *self.session = Session::default();
                }
                // Discord asks for a pause of one to five seconds here.
                self.inner.pause(self.inner.options.reconnect_base.mul_f64(1.0 + 4.0 * jitter()));
                return Ok(Some(Exit::Reconnect));
            }
            Some(OP_HELLO) => {
                self.interval = Duration::from_millis(value["d"]["heartbeat_interval"].as_u64().unwrap_or(41_250));
                self.next_beat = Some(Instant::now() + self.interval.mul_f64(jitter()));
                self.awaiting_ack = false;
                match self.session.id.clone() {
                    Some(id) => {
                        let seq = self.session.seq;
                        self.resume(id, seq)?;
                    }
                    None => self.identify()?,
                }
            }
            _ => {}
        }
        Ok(None)
    }
    fn on_close(&mut self, code: u16) -> Exit {
        if self.inner.is_debug() {
            host::log(&self.inner.host, Level::Info, &format!("Discord closed the gateway with {code}."));
        }
        match classify(code) {
            CloseAction::Fatal(reason) => Exit::Fatal(reason),
            CloseAction::Fresh => {
                *self.session = Session::default();
                Exit::Reconnect
            }
            CloseAction::Resume => Exit::Reconnect,
        }
    }
    fn run(&mut self) -> Exit {
        loop {
            if self.inner.stop.load(Ordering::SeqCst) {
                let _ = self.socket.close(Some(CloseFrame { code: CloseCode::Normal, reason: "".into() }));
                // The close frame goes out, and the far end gets a moment to answer.
                let until = Instant::now() + Duration::from_millis(500);
                while Instant::now() < until && self.socket.read().is_ok() {}
                return Exit::Stop;
            }
            for line in self.inner.take_outbox() {
                if self.send(&line).is_err() {
                    return Exit::Reconnect;
                }
            }
            if self.next_beat.is_some_and(|at| Instant::now() >= at) {
                if self.awaiting_ack {
                    // A zombie: no ack since the last beat.
                    let _ = self.socket.close(Some(CloseFrame { code: CloseCode::Library(4900), reason: "".into() }));
                    return Exit::Reconnect;
                }
                self.awaiting_ack = true;
                self.next_beat = Some(Instant::now() + self.interval);
                if self.heartbeat().is_err() {
                    return Exit::Reconnect;
                }
            }
            if !self.pending_guilds.is_empty() {
                self.check_ready();
            }
            match self.socket.read() {
                Ok(Frame::Text(text)) => match self.on_frame(text.as_str()) {
                    Ok(Some(exit)) => {
                        let _ = self.socket.close(Some(CloseFrame { code: CloseCode::Library(4900), reason: "".into() }));
                        return exit;
                    }
                    Ok(None) => {}
                    Err(_) => return Exit::Reconnect,
                },
                Ok(Frame::Close(frame)) => return self.on_close(frame.map_or(1006, |frame| frame.code.into())),
                Ok(_) => {
                    let _ = self.socket.flush();
                }
                Err(tungstenite::Error::Io(error)) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    let _ = self.socket.flush();
                }
                Err(_) => return Exit::Reconnect,
            }
        }
    }
}

/// The thread: connections, one after another, until told to stop or a close code ends it.
pub(super) fn run(inner: Arc<Inner>, url: String) {
    let url = format!("{}/?v=10&encoding=json", url.trim_end_matches('/'));
    let mut session = Session::default();
    let mut failures = 0;
    let mut last_identify = None;
    let mut pending_guilds = HashSet::new();
    let mut ready_since = None;
    while !inner.stop.load(Ordering::SeqCst) {
        let target = session.resume_url.as_ref().filter(|_| session.id.is_some()).map_or_else(|| url.clone(), |resume| format!("{}/?v=10&encoding=json", resume.trim_end_matches('/')));
        match connect(&target, inner.options.proxy.as_deref(), &inner.options.extra_roots) {
            Ok(socket) => {
                let mut connection = Connection {
                    inner: &inner,
                    socket,
                    session: &mut session,
                    next_beat: None,
                    interval: Duration::from_secs(41),
                    awaiting_ack: false,
                    last_identify: &mut last_identify,
                    pending_guilds: &mut pending_guilds,
                    ready_since: &mut ready_since,
                    failures: &mut failures,
                };
                match connection.run() {
                    Exit::Stop => break,
                    Exit::Fatal(reason) => {
                        inner.gateway_failed(reason);
                        break;
                    }
                    Exit::Reconnect => {}
                }
            }
            Err(reason) => {
                if inner.is_debug() {
                    host::log(&inner.host, Level::Warning, &format!("Couldn't reach the Discord gateway: {reason}"));
                }
            }
        }
        inner.pause(backoff(inner.options.reconnect_base, failures));
        failures += 1;
    }
}

/// What the gateway's events change in what's kept. Message and interaction events change nothing.
pub(super) fn apply(cache: &mut super::Cache, kind: &str, data: &Value) {
    let guild_id = snowflake(&data["guild_id"]);
    match kind {
        "GUILD_CREATE" => {
            if let Some(guild) = Guild::from_json(data, cache.self_user.as_ref().map(|user| user.id)) {
                cache.guilds.insert(guild.id, guild);
            }
        }
        "GUILD_UPDATE" => {
            if let Some(guild) = snowflake(&data["id"]).and_then(|id| cache.guilds.get_mut(&id)) {
                guild.update(data);
            }
        }
        "GUILD_DELETE" => {
            if let Some(id) = snowflake(&data["id"]) {
                cache.guilds.remove(&id);
            }
        }
        "GUILD_ROLE_CREATE" | "GUILD_ROLE_UPDATE" => {
            if let (Some(guild), Some(role)) = (guild_id.and_then(|id| cache.guilds.get_mut(&id)), Role::from_json(&data["role"])) {
                guild.roles.insert(role.id, role);
            }
        }
        "GUILD_ROLE_DELETE" => {
            if let (Some(guild), Some(role)) = (guild_id.and_then(|id| cache.guilds.get_mut(&id)), snowflake(&data["role_id"])) {
                guild.roles.remove(&role);
            }
        }
        "CHANNEL_CREATE" | "CHANNEL_UPDATE" => {
            if let (Some(guild), Some(channel)) = (guild_id.and_then(|id| cache.guilds.get_mut(&id)), super::model::Channel::from_json(data)) {
                guild.channels.insert(channel.id, channel);
            }
        }
        "CHANNEL_DELETE" => {
            if let (Some(guild), Some(channel)) = (guild_id.and_then(|id| cache.guilds.get_mut(&id)), snowflake(&data["id"])) {
                guild.channels.remove(&channel);
            }
        }
        "GUILD_EMOJIS_UPDATE" => {
            if let Some(guild) = guild_id.and_then(|id| cache.guilds.get_mut(&id)) {
                guild.emojis = data["emojis"].as_array().into_iter().flatten().filter_map(super::model::Emoji::from_json).collect();
            }
        }
        // Sent for the bot itself whatever the intents: its roles decide what it may do.
        "GUILD_MEMBER_UPDATE" => {
            let me = cache.self_user.as_ref().map(|user| user.id);
            if let (Some(guild), Some(member)) = (guild_id.and_then(|id| cache.guilds.get_mut(&id)), Member::from_json(data, None)) {
                if Some(member.user.id) == me {
                    guild.self_member = Some(member);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_codes_that_stop_retrying() {
        for code in [4004, 4010, 4011, 4012, 4013, 4014] {
            assert!(matches!(classify(code), CloseAction::Fatal(_)), "{code}");
        }
        for code in [1000, 1001, 4007, 4009] {
            assert_eq!(classify(code), CloseAction::Fresh, "{code}");
        }
        for code in [1006, 4000, 4001, 4002, 4003, 4005, 4008] {
            assert_eq!(classify(code), CloseAction::Resume, "{code}");
        }
    }

    #[test]
    fn backoff_doubles_and_stops_growing() {
        let base = Duration::from_secs(1);
        assert_eq!(backoff(base, 0), base);
        assert_eq!(backoff(base, 3), Duration::from_secs(8));
        assert_eq!(backoff(base, 40), MAX_BACKOFF);
    }

    #[test]
    fn addresses() {
        assert_eq!(host_and_port("wss://gateway.discord.gg/?v=10&encoding=json"), Some(("gateway.discord.gg".to_owned(), 443)));
        assert_eq!(host_and_port("ws://127.0.0.1:9000/?v=10"), Some(("127.0.0.1".to_owned(), 9000)));
        assert_eq!(proxy_address("proxy.local:3128"), Some(("proxy.local".to_owned(), 3128)));
        assert_eq!(proxy_address("http://proxy.local:3128/"), Some(("proxy.local".to_owned(), 3128)));
    }

    #[test]
    fn the_tls_setup_does_not_panic() {
        assert!(matches!(tls_connector(&[]), Connector::Rustls(_)));
    }

    #[test]
    fn a_proxy_that_says_yes_gives_a_stream_and_one_that_says_no_does_not() {
        for (answer, works) in [("HTTP/1.1 200 Connection established\r\n\r\n", true), ("HTTP/1.1 407 Proxy Authentication Required\r\n\r\n", false)] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut asked = [0u8; 256];
                let read = stream.read(&mut asked).unwrap();
                stream.write_all(answer.as_bytes()).unwrap();
                String::from_utf8_lossy(&asked[..read]).into_owned()
            });
            let result = open_through(&format!("127.0.0.1:{port}"), "gateway.example", 443);
            assert_eq!(result.is_ok(), works);
            assert!(server.join().unwrap().starts_with("CONNECT gateway.example:443 HTTP/1.1"));
        }
    }
}
