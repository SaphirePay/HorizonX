//! The little of XMPP that `XMPPManager` used Smack for: connect, STARTTLS, log in, keep a stream
//! open. All of it runs on one thread, which owns the socket.
use essentials::prelude::*;
use crate::xml::{Element, Event, Parser, escape};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use ring::{digest, hmac, pbkdf2, rand::SecureRandom, rand::SystemRandom};
use rustls::pki_types::ServerName;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};

const STREAMS: &str = "http://etherx.jabber.org/streams";
const STEP: Duration = Duration::from_secs(15);
const RESOURCE: &str = "Essentials-XMPP";

#[derive(Clone)]
pub struct Settings {
    pub server: String,
    pub port: u16,
    pub service: String,
    pub user: String,
    pub password: String,
    pub sasl: bool,
    pub require_tls: bool,
    /// Who to trust: the public roots unless a test says otherwise.
    pub roots: Option<Arc<rustls::RootCertStore>>,
}

impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Settings({}:{} as {})", self.server, self.port, self.user)
    }
}

/// How far a failed login got: Java logs all of this.
#[derive(Debug, Default)]
pub struct Failure {
    pub reason: String,
    pub connected: bool,
    pub secure: bool,
    pub authenticated: bool,
}

enum Link {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}

impl Read for Link {
    fn read(&mut self, into: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.read(into),
            Self::Tls(stream) => stream.read(into),
        }
    }
}

impl Write for Link {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.write(bytes),
            Self::Tls(stream) => stream.write(bytes),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Plain(stream) => stream.flush(),
            Self::Tls(stream) => stream.flush(),
        }
    }
}

/// What the rest of the module hears.
#[derive(Debug, PartialEq, Eq)]
pub enum Incoming {
    Message { from: String, body: String },
}

pub struct Session {
    link: Link,
    parser: Parser,
    next_id: u64,
    pub secure: bool,
    stream_id: String,
}

fn presence_xml(status: &str, dnd: bool) -> String {
    let show = if dnd { "<show>dnd</show>" } else { "" };
    format!("<presence><status>{}</status><priority>2</priority>{show}</presence>", escape(status))
}

impl Session {
    fn write(&mut self, text: &str) -> Result<(), String> {
        self.link.write_all(text.as_bytes()).and_then(|()| self.link.flush()).map_err(|e| format!("the connection failed: {e}"))
    }

    fn read_some(&mut self) -> Result<bool, String> {
        let mut chunk = [0u8; 8192];
        match self.link.read(&mut chunk) {
            Ok(0) => Err("the server closed the connection".to_owned()),
            Ok(count) => {
                self.parser.feed(&chunk[..count])?;
                Ok(true)
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => Ok(false),
            Err(e) => Err(format!("the connection failed: {e}")),
        }
    }

    /// The next event, waiting up to the time of a login step.
    fn wait(&mut self) -> Result<Event, String> {
        let deadline = Instant::now() + STEP;
        loop {
            if let Some(event) = self.parser.next()? {
                return Ok(event);
            }
            fail_if!(Instant::now() >= deadline, "the server didn't answer in time".to_owned());
            self.read_some()?;
        }
    }

    fn wait_stanza(&mut self) -> Result<Element, String> {
        match self.wait()? {
            Event::Stanza(stanza) if stanza.name == "error" => Err(stream_error(&stanza)),
            Event::Stanza(stanza) => Ok(stanza),
            Event::Open(_) => Err("the server opened a second stream".to_owned()),
            Event::Close => Err("the server closed the stream".to_owned()),
        }
    }

    /// A new stream: returns its features.
    fn open(&mut self, service: &str) -> Result<Element, String> {
        self.parser.restart();
        self.write(&format!(
            "<?xml version='1.0' encoding='UTF-8'?><stream:stream to='{}' xmlns='jabber:client' xmlns:stream='{STREAMS}' version='1.0'>",
            escape(service)
        ))?;
        match self.wait()? {
            Event::Open(stream) => self.stream_id = stream.attr("id").unwrap_or_default().to_owned(),
            other => return Err(format!("the server didn't open a stream: {other:?}")),
        }
        let features = self.wait_stanza()?;
        if features.name == "features" { Ok(features) } else { Err(format!("expected the stream's features, got <{}>", features.name)) }
    }

    fn new_id(&mut self) -> String {
        self.next_id += 1;
        format!("ess{}", self.next_id)
    }

    /// An `<iq>` and its answer, skipping anything that arrives first.
    fn iq(&mut self, kind: &str, inner: &str) -> Result<Element, String> {
        let id = self.new_id();
        self.write(&format!("<iq type='{kind}' id='{id}'>{inner}</iq>"))?;
        loop {
            let stanza = self.wait_stanza()?;
            if stanza.name == "iq" && stanza.attr("id") == Some(&id) {
                return Ok(stanza);
            }
        }
    }

    fn start_tls(self, settings: &Settings) -> Result<Self, String> {
        let Self { link, parser, next_id, stream_id, .. } = self;
        let Link::Plain(socket) = link else {
            return Err("TLS was started twice".to_owned());
        };
        let roots = settings.roots.clone().unwrap_or_else(|| {
            let mut store = rustls::RootCertStore::empty();
            store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            Arc::new(store)
        });
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_root_certificates(roots)
            .with_no_client_auth();
        let name = ServerName::try_from(settings.service.clone()).map_err(|e| format!("bad service name: {e}"))?;
        let connection = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| e.to_string())?;
        let mut stream = rustls::StreamOwned::new(connection, socket);
        // The handshake, now: a bad certificate must not reach the login.
        while stream.conn.is_handshaking() {
            match stream.conn.complete_io(&mut stream.sock) {
                Ok(_) => {}
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => continue,
                Err(e) => return Err(format!("TLS failed: {e}")),
            }
        }
        Ok(Self { link: Link::Tls(Box::new(stream)), parser, next_id, secure: true, stream_id })
    }

    fn sasl(&mut self, features: &Element, settings: &Settings) -> Result<(), String> {
        let mechanisms: Vec<&str> = features.child("mechanisms").map(|list| list.children.iter().map(|m| m.text.trim()).collect()).unwrap_or_default();
        if mechanisms.contains(&"SCRAM-SHA-1") {
            return self.scram(settings);
        }
        if mechanisms.contains(&"PLAIN") {
            let token = BASE64.encode(format!("\0{}\0{}", settings.user, settings.password));
            self.write(&format!("<auth xmlns='urn:ietf:params:xml:ns:xmpp-sasl' mechanism='PLAIN'>{token}</auth>"))?;
            return match self.wait_stanza()? {
                reply if reply.name == "success" => Ok(()),
                reply => Err(sasl_failure(&reply)),
            };
        }
        Err(format!("no login method both sides know: the server offers {mechanisms:?}"))
    }

    /// RFC 5802, which servers kept when DIGEST-MD5, Smack's choice, was dropped.
    fn scram(&mut self, settings: &Settings) -> Result<(), String> {
        let mut random = [0u8; 18];
        SystemRandom::new().fill(&mut random).map_err(|_| "no randomness".to_owned())?;
        let nonce = BASE64.encode(random);
        let name = settings.user.replace('=', "=3D").replace(',', "=2C");
        let first = format!("n={name},r={nonce}");
        let token = BASE64.encode(format!("n,,{first}"));
        self.write(&format!("<auth xmlns='urn:ietf:params:xml:ns:xmpp-sasl' mechanism='SCRAM-SHA-1'>{token}</auth>"))?;
        let challenge = self.wait_stanza()?;
        fail_if!(challenge.name != "challenge", sasl_failure(&challenge));
        let server_first = BASE64.decode(challenge.text.trim()).ok().and_then(|bytes| String::from_utf8(bytes).ok()).ok_or("a bad challenge")?;
        let (response, expected) = scram_answer(&server_first, &nonce, &first, &settings.password)?;
        self.write(&format!("<response xmlns='urn:ietf:params:xml:ns:xmpp-sasl'>{}</response>", BASE64.encode(response)))?;
        let done = self.wait_stanza()?;
        fail_if!(done.name != "success", sasl_failure(&done));
        let signature = BASE64.decode(done.text.trim()).unwrap_or_default();
        fail_if!(signature != expected.into_bytes(), "the server couldn't prove it knows the password".to_owned());
        Ok(())
    }

    /// XEP-0078, which `sasl-enabled: false` asks for, Java's default.
    fn legacy(&mut self, settings: &Settings) -> Result<(), String> {
        let user = escape(&settings.user);
        let offer = self.iq("get", &format!("<query xmlns='jabber:iq:auth'><username>{user}</username></query>"))?;
        fail_if!(offer.attr("type") != Some("result"), "the server doesn't take this old kind of login: try `sasl-enabled: true`".to_owned());
        let query = offer.child("query").ok_or("no login fields")?;
        let proof = if query.child("digest").is_some() {
            let digest = digest::digest(&digest::SHA1_FOR_LEGACY_USE_ONLY, format!("{}{}", self.stream_id, settings.password).as_bytes());
            format!("<digest>{}</digest>", hex(digest.as_ref()))
        } else {
            format!("<password>{}</password>", escape(&settings.password))
        };
        let answer = self.iq("set", &format!("<query xmlns='jabber:iq:auth'><username>{user}</username>{proof}<resource>{RESOURCE}</resource></query>"))?;
        if answer.attr("type") == Some("result") { Ok(()) } else { Err("the server refused the login".to_owned()) }
    }

    fn bind(&mut self, features: &Element) -> Result<(), String> {
        fail_if!(features.child("bind").is_none(), "the server has no resource binding".to_owned());
        let bound = self.iq("set", &format!("<bind xmlns='urn:ietf:params:xml:ns:xmpp-bind'><resource>{RESOURCE}</resource></bind>"))?;
        fail_if!(bound.attr("type") != Some("result"), "the server refused the resource".to_owned());
        if let Some(session) = features.child("session")
            && session.child("optional").is_none()
        {
            let started = self.iq("set", "<session xmlns='urn:ietf:params:xml:ns:xmpp-session'/>")?;
            fail_if!(started.attr("type") != Some("result"), "the server refused the session".to_owned());
        }
        Ok(())
    }

    pub fn message(&mut self, to: &str, body: &str) -> Result<(), String> {
        self.write(&format!("<message to='{}' type='chat'><body>{}</body></message>", escape(to), escape(body)))
    }

    /// `new Presence(available, status, 2, mode)`.
    pub fn presence(&mut self, status: &str, dnd: bool) -> Result<(), String> {
        self.write(&presence_xml(status, dnd))
    }

    /// Smack's keep-alive: one space.
    pub fn keep_alive(&mut self) -> Result<(), String> {
        self.write(" ")
    }

    /// `connection.disconnect(new Presence(unavailable))`.
    pub fn close(&mut self) {
        let _ = self.write("<presence type='unavailable'/></stream:stream>");
        if let Link::Tls(stream) = &mut self.link {
            stream.conn.send_close_notify();
            let _ = stream.conn.write_tls(&mut stream.sock);
        }
    }

    /// What has arrived, waiting no longer than the socket's short timeout.
    pub fn poll(&mut self) -> Result<Vec<Incoming>, String> {
        self.read_some()?;
        let mut heard = Vec::new();
        while let Some(event) = self.parser.next()? {
            match event {
                Event::Stanza(stanza) => match stanza.name.as_str() {
                    "message" => heard.extend(read_message(&stanza)),
                    "presence" => self.answer_presence(&stanza)?,
                    "iq" => self.answer_iq(&stanza)?,
                    "error" => return Err(stream_error(&stanza)),
                    _ => {}
                },
                Event::Close => return Err("the server closed the stream".to_owned()),
                Event::Open(_) => {}
            }
        }
        Ok(heard)
    }

    /// `SubscriptionMode.accept_all`.
    fn answer_presence(&mut self, stanza: &Element) -> Result<(), String> {
        if stanza.attr("type") == Some("subscribe")
            && let Some(from) = stanza.attr("from")
        {
            self.write(&format!("<presence to='{}' type='subscribed'/>", escape(from)))?;
        }
        Ok(())
    }

    /// A ping gets its answer, and any other request a refusal: RFC 6120 wants one, and a server
    /// may drop a client that gives none.
    fn answer_iq(&mut self, stanza: &Element) -> Result<(), String> {
        if !matches!(stanza.attr("type"), Some("get" | "set")) {
            return Ok(());
        }
        let id = escape(stanza.attr("id").unwrap_or_default());
        let to = stanza.attr("from").map(|from| format!(" to='{}'", escape(from))).unwrap_or_default();
        if stanza.child("ping").is_some() {
            return self.write(&format!("<iq type='result' id='{id}'{to}/>"));
        }
        self.write(&format!(
            "<iq type='error' id='{id}'{to}><error type='cancel'><service-unavailable xmlns='urn:ietf:params:xml:ns:xmpp-stanzas'/></error></iq>"
        ))
    }
}

/// `ChatManager`'s filter and `processMessage`'s first line.
fn read_message(stanza: &Element) -> Option<Incoming> {
    if matches!(stanza.attr("type"), Some("error" | "groupchat" | "headline")) {
        return None;
    }
    let from = stanza.attr("from")?.to_owned();
    let body = stanza.child_text("body")?.to_owned();
    (!body.is_empty()).then_some(Incoming::Message { from, body })
}

fn stream_error(stanza: &Element) -> String {
    let what = stanza.children.iter().map(|child| child.name.as_str()).find(|name| *name != "text").unwrap_or("unknown");
    format!("stream error: {what}")
}

fn sasl_failure(stanza: &Element) -> String {
    let what = stanza.children.iter().map(|child| child.name.as_str()).find(|name| *name != "text").unwrap_or(&stanza.name);
    format!("the server refused the login: {what}")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `client-final-message` and the `v=` the server must send back.
fn scram_answer(server_first: &str, nonce: &str, client_first: &str, password: &str) -> Result<(String, String), String> {
    let field = |key: &str| server_first.split(',').find_map(|part| part.strip_prefix(key)).map(str::to_owned);
    let combined = field("r=").ok_or("a challenge with no nonce")?;
    fail_if!(!combined.starts_with(nonce) || combined.len() == nonce.len(), "the server changed the login nonce".to_owned());
    let salt = BASE64.decode(field("s=").ok_or("a challenge with no salt")?).map_err(|_| "a bad salt".to_owned())?;
    let rounds: u32 = field("i=").and_then(|count| count.parse().ok()).ok_or("a challenge with no count")?;
    // Too few gives the password away; too many hangs this thread.
    fail_if!(!(1000..=10_000_000).contains(&rounds), format!("the server asked for {rounds} rounds"));
    let mut salted = [0u8; 20];
    let count = NonZeroU32::new(rounds).ok_or("a count of nothing")?;
    pbkdf2::derive(pbkdf2::PBKDF2_HMAC_SHA1, count, &salt, password.as_bytes(), &mut salted);
    let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, &salted);
    let client_key = hmac::sign(&key, b"Client Key");
    let stored = digest::digest(&digest::SHA1_FOR_LEGACY_USE_ONLY, client_key.as_ref());
    let without_proof = format!("c=biws,r={combined}");
    let message = format!("{client_first},{server_first},{without_proof}");
    let signature = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, stored.as_ref()), message.as_bytes());
    let proof: Vec<u8> = client_key.as_ref().iter().zip(signature.as_ref()).map(|(a, b)| a ^ b).collect();
    let server_key = hmac::sign(&key, b"Server Key");
    let server_signature = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, server_key.as_ref()), message.as_bytes());
    Ok((format!("{without_proof},p={}", BASE64.encode(proof)), format!("v={}", BASE64.encode(server_signature.as_ref()))))
}

/// The steps of `login` on an open socket; `progress` is how far they got, for the log.
fn establish(mut session: Session, settings: &Settings, progress: &mut (bool, bool)) -> Result<Session, String> {
    let mut features = session.open(&settings.service)?;
    if features.child("starttls").is_some() {
        session.write("<starttls xmlns='urn:ietf:params:xml:ns:xmpp-tls'/>")?;
        let answer = session.wait_stanza()?;
        fail_if!(answer.name != "proceed", "the server wouldn't start TLS".to_owned());
        session = session.start_tls(settings)?;
        progress.0 = true;
        features = session.open(&settings.service)?;
    } else if settings.require_tls {
        return Err("TLS is required, and the server doesn't offer it".to_owned());
    }
    if settings.sasl && features.child("mechanisms").is_some() {
        session.sasl(&features, settings)?;
        progress.1 = true;
        features = session.open(&settings.service)?;
        session.bind(&features)?;
    } else {
        session.legacy(settings)?;
        progress.1 = true;
    }
    session.write("<presence/>")?;
    Ok(session)
}

/// `connection.connect()` and `connection.login(user, password, "Essentials-XMPP")`, with the
/// first presence `setSendPresence(true)` asks for.
pub fn login(settings: &Settings) -> Result<Session, Failure> {
    let fail = |reason: String| Failure { reason, ..Failure::default() };
    let address = (settings.server.as_str(), settings.port).to_socket_addrs().map_err(|e| fail(e.to_string()))?.next().ok_or_else(|| fail("no address".to_owned()))?;
    let socket = TcpStream::connect_timeout(&address, STEP).map_err(|e| fail(e.to_string()))?;
    let _ = socket.set_nodelay(true);
    let _ = socket.set_write_timeout(Some(STEP));
    socket.set_read_timeout(Some(Duration::from_millis(50))).map_err(|e| fail(e.to_string()))?;
    let session = Session { link: Link::Plain(socket), parser: Parser::default(), next_id: 0, secure: false, stream_id: String::new() };
    let mut progress = (false, false);
    establish(session, settings, &mut progress).map_err(|reason| Failure { reason, connected: true, secure: progress.0, authenticated: progress.1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 5802's example: user `user`, password `pencil`.
    #[test]
    fn scram_matches_the_rfc() {
        let first = "n=user,r=fyko+d2lbbFgONRv9qkxdawL";
        let server = "r=fyko+d2lbbFgONRv9qkxdawL3rfcNHYJY1ZVvWVs7j,s=QSXCR+Q6sek8bf92,i=4096";
        let (answer, verify) = scram_answer(server, "fyko+d2lbbFgONRv9qkxdawL", first, "pencil").unwrap();
        assert_eq!(answer, "c=biws,r=fyko+d2lbbFgONRv9qkxdawL3rfcNHYJY1ZVvWVs7j,p=v0X8v3Bz2T0CJGbJQyF0X+HI4Ts=");
        assert_eq!(verify, "v=rmF9pqV8S7suAoZWja4dJRkFsKQ=");
    }

    #[test]
    fn scram_refuses_a_nonce_it_did_not_start() {
        assert!(scram_answer("r=other,s=QSXCR+Q6sek8bf92,i=4096", "mine", "n=u,r=mine", "p").is_err());
        assert!(scram_answer("r=mine,s=QSXCR+Q6sek8bf92,i=4096", "mine", "n=u,r=mine", "p").is_err());
    }

    #[test]
    fn scram_refuses_a_count_that_gives_the_password_away() {
        assert!(scram_answer("r=minex,s=QSXCR+Q6sek8bf92,i=1", "mine", "n=u,r=mine", "p").is_err());
    }

    #[test]
    fn presence_is_what_java_sends() {
        assert_eq!(presence_xml("No one online.", true), "<presence><status>No one online.</status><priority>2</priority><show>dnd</show></presence>");
        assert_eq!(presence_xml("1 player online.", false), "<presence><status>1 player online.</status><priority>2</priority></presence>");
    }

    #[test]
    fn the_password_is_not_in_debug_output() {
        let settings = Settings { server: "s".into(), port: 1, service: "s".into(), user: "u".into(), password: "secret".into(), sasl: true, require_tls: false, roots: None };
        assert!(!format!("{settings:?}").contains("secret"));
    }

    #[test]
    fn only_chat_with_a_body_is_heard() {
        let message = |kind: &str, body: bool| Element {
            name: "message".into(),
            attrs: vec![("from".into(), "a@b/c".into()), ("type".into(), kind.into())],
            children: if body { vec![Element { name: "body".into(), text: "hi".into(), ..Element::default() }] } else { Vec::new() },
            text: String::new(),
        };
        assert_eq!(read_message(&message("chat", true)), Some(Incoming::Message { from: "a@b/c".into(), body: "hi".into() }));
        assert_eq!(read_message(&message("chat", false)), None);
        for kind in ["error", "groupchat", "headline"] {
            assert_eq!(read_message(&message(kind, true)), None);
        }
    }
}
