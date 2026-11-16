//! `client` against a small server on this machine, speaking what a real one speaks. No real XMPP
//! server has been tried.
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use essentials_xmpp::client::{Incoming, Session, Settings, login};
use essentials_xmpp::xml::{Element, Event, Parser};
use ring::{digest, hmac, pbkdf2};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::num::NonZeroU32;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const OPEN: &str = "<?xml version='1.0'?><stream:stream from='local.test' id='stream1' xmlns='jabber:client' xmlns:stream='http://etherx.jabber.org/streams' version='1.0'>";
const BIND: &str = "<bind xmlns='urn:ietf:params:xml:ns:xmpp-bind'/>";

fn read_event<S: Read>(stream: &mut S, parser: &mut Parser) -> Event {
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(event) = parser.next().unwrap() {
            return event;
        }
        let count = stream.read(&mut chunk).unwrap();
        assert!(count > 0, "the client went away");
        parser.feed(&chunk[..count]).unwrap();
    }
}

fn read_stanza<S: Read>(stream: &mut S, parser: &mut Parser) -> Element {
    match read_event(stream, parser) {
        Event::Stanza(stanza) => stanza,
        other => panic!("expected a stanza, got {other:?}"),
    }
}

fn open<S: Read + Write>(stream: &mut S, parser: &mut Parser, features: &str) {
    parser.restart();
    assert!(matches!(read_event(stream, parser), Event::Open(_)));
    stream.write_all(format!("{OPEN}<stream:features>{features}</stream:features>").as_bytes()).unwrap();
}

/// After login: echoes what it's told, and says what the test queues.
fn play<S: Read + Write>(stream: &mut S, parser: &mut Parser, told: &Sender<Element>, say: &Receiver<String>) {
    while let Ok(line) = say.try_recv() {
        stream.write_all(line.as_bytes()).unwrap();
    }
    let _ = stream.flush();
    let mut chunk = [0u8; 4096];
    loop {
        while let Ok(Some(event)) = parser.next() {
            match event {
                Event::Stanza(stanza) => {
                    if told.send(stanza).is_err() {
                        return;
                    }
                }
                _ => return,
            }
        }
        while let Ok(line) = say.try_recv() {
            if line == "DROP" {
                return;
            }
            let _ = stream.write_all(line.as_bytes());
            let _ = stream.flush();
        }
        match stream.read(&mut chunk) {
            Ok(0) => return,
            Ok(count) => parser.feed(&chunk[..count]).unwrap(),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => return,
        }
    }
}

/// `bind`, after a login's stream restart.
fn bind_and_play<S: Read + Write>(mut stream: S, parser: &mut Parser, told: &Sender<Element>, say: &Receiver<String>) {
    open(&mut stream, parser, BIND);
    let iq = read_stanza(&mut stream, parser);
    assert!(iq.child("bind").is_some());
    stream.write_all(format!("<iq type='result' id='{}'><bind xmlns='urn:ietf:params:xml:ns:xmpp-bind'><jid>u@local.test/r</jid></bind></iq>", iq.attr("id").unwrap()).as_bytes()).unwrap();
    play(&mut stream, parser, told, say);
}

struct Server {
    port: u16,
    told: Receiver<Element>,
    say: Sender<String>,
}

fn serve(run: impl FnOnce(TcpStream, Sender<Element>, Receiver<String>) + Send + 'static) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (told_in, told) = channel();
    let (say, say_out) = channel();
    thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
        run(stream, told_in, say_out);
    });
    Server { port, told, say }
}

fn blocking(stream: &TcpStream) {
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
}

fn settings(port: u16, sasl: bool) -> Settings {
    Settings {
        server: "127.0.0.1".into(),
        port,
        service: "localhost".into(),
        user: "user".into(),
        password: "pencil".into(),
        sasl,
        require_tls: false,
        roots: None,
    }
}

fn next_told(server: &Server, name: &str) -> Element {
    loop {
        let stanza = server.told.recv_timeout(Duration::from_secs(5)).unwrap_or_else(|_| panic!("no <{name}> from the client"));
        if stanza.name == name {
            return stanza;
        }
    }
}

fn poll_until(session: &mut Session) -> Vec<Incoming> {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        let heard = session.poll().unwrap();
        if !heard.is_empty() {
            return heard;
        }
    }
    panic!("nothing heard");
}

fn sasl_plain_server() -> Server {
    serve(|mut stream, told, say| {
        blocking(&stream);
        let mut parser = Parser::default();
        open(&mut stream, &mut parser, "<mechanisms xmlns='urn:ietf:params:xml:ns:xmpp-sasl'><mechanism>PLAIN</mechanism></mechanisms>");
        let auth = read_stanza(&mut stream, &mut parser);
        assert_eq!(auth.attr("mechanism"), Some("PLAIN"));
        assert_eq!(BASE64.decode(auth.text.trim()).unwrap(), b"\0user\0pencil");
        stream.write_all(b"<success xmlns='urn:ietf:params:xml:ns:xmpp-sasl'/>").unwrap();
        stream.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
        bind_and_play(stream, &mut parser, &told, &say);
    })
}

#[test]
fn plain_login_then_messages_both_ways() {
    let server = sasl_plain_server();
    let mut session = login(&settings(server.port, true)).unwrap();
    assert!(!session.secure);
    next_told(&server, "presence");
    session.message("a@b.c", "1 < 2 & \u{1B}[0mfine").unwrap();
    let sent = next_told(&server, "message");
    assert_eq!(sent.attr("to"), Some("a@b.c"));
    assert_eq!(sent.attr("type"), Some("chat"));
    assert_eq!(sent.child_text("body"), Some("1 < 2 & [0mfine"));
    server.say.send("<message from='a@b.c/phone' type='chat'><body>hello &amp; hi</body></message>".into()).unwrap();
    assert_eq!(poll_until(&mut session), vec![Incoming::Message { from: "a@b.c/phone".into(), body: "hello & hi".into() }]);
}

#[test]
fn subscriptions_are_accepted_and_pings_answered() {
    let server = sasl_plain_server();
    let mut session = login(&settings(server.port, true)).unwrap();
    server.say.send("<presence from='friend@b.c' type='subscribe'/><iq type='get' id='p1' from='b.c'><ping xmlns='urn:xmpp:ping'/></iq><iq type='get' id='v1' from='b.c'><query xmlns='jabber:iq:version'/></iq><message from='a@b.c' type='chat'><body>done</body></message>".into()).unwrap();
    poll_until(&mut session);
    let mut subscribed = false;
    let mut pinged = false;
    let mut refused = false;
    while !(subscribed && pinged && refused) {
        let stanza = server.told.recv_timeout(Duration::from_secs(5)).expect("an answer");
        subscribed |= stanza.name == "presence" && stanza.attr("type") == Some("subscribed") && stanza.attr("to") == Some("friend@b.c");
        pinged |= stanza.name == "iq" && stanza.attr("id") == Some("p1") && stanza.attr("type") == Some("result");
        refused |= stanza.name == "iq" && stanza.attr("id") == Some("v1") && stanza.attr("type") == Some("error");
    }
}

#[test]
fn a_dropped_connection_is_an_error_not_a_hang() {
    let server = sasl_plain_server();
    let mut session = login(&settings(server.port, true)).unwrap();
    next_told(&server, "presence");
    server.say.send("DROP".into()).unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        match session.poll() {
            Err(_) => break,
            Ok(_) => assert!(Instant::now() < end, "the drop wasn't noticed"),
        }
    }
}

#[test]
fn a_refused_login_says_so_and_how_far_it_got() {
    let server = serve(|mut stream, _told, _say| {
        blocking(&stream);
        let mut parser = Parser::default();
        open(&mut stream, &mut parser, "<mechanisms xmlns='urn:ietf:params:xml:ns:xmpp-sasl'><mechanism>PLAIN</mechanism></mechanisms>");
        read_stanza(&mut stream, &mut parser);
        stream.write_all(b"<failure xmlns='urn:ietf:params:xml:ns:xmpp-sasl'><not-authorized/></failure>").unwrap();
        thread::sleep(Duration::from_secs(1));
    });
    let Err(failure) = login(&settings(server.port, true)) else { panic!("logged in") };
    assert!(failure.reason.contains("not-authorized"), "{}", failure.reason);
    assert!(failure.connected && !failure.authenticated);
    assert!(!failure.reason.contains("pencil"));
}

#[test]
fn tls_is_required_when_asked_for() {
    let server = serve(|mut stream, _told, _say| {
        blocking(&stream);
        let mut parser = Parser::default();
        open(&mut stream, &mut parser, "<mechanisms xmlns='urn:ietf:params:xml:ns:xmpp-sasl'><mechanism>PLAIN</mechanism></mechanisms>");
        thread::sleep(Duration::from_secs(1));
    });
    let mut wanted = settings(server.port, true);
    wanted.require_tls = true;
    let Err(failure) = login(&wanted) else { panic!("logged in without TLS") };
    assert!(failure.reason.contains("TLS"), "{}", failure.reason);
    assert!(!failure.authenticated);
}

#[test]
fn legacy_login_uses_the_digest_it_is_offered() {
    let server = serve(|mut stream, told, say| {
        blocking(&stream);
        let mut parser = Parser::default();
        open(&mut stream, &mut parser, "");
        let get = read_stanza(&mut stream, &mut parser);
        stream.write_all(format!("<iq type='result' id='{}'><query xmlns='jabber:iq:auth'><username/><digest/><password/><resource/></query></iq>", get.attr("id").unwrap()).as_bytes()).unwrap();
        let set = read_stanza(&mut stream, &mut parser);
        let query = set.child("query").unwrap();
        let want = digest::digest(&digest::SHA1_FOR_LEGACY_USE_ONLY, b"stream1pencil");
        let want: String = want.as_ref().iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(query.child_text("digest"), Some(want.as_str()));
        assert_eq!(query.child_text("resource"), Some("Essentials-XMPP"));
        assert!(query.child("password").is_none());
        stream.write_all(format!("<iq type='result' id='{}'/>", set.attr("id").unwrap()).as_bytes()).unwrap();
        stream.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
        play(&mut stream, &mut parser, &told, &say);
    });
    let _session = login(&settings(server.port, false)).unwrap();
    next_told(&server, "presence");
}

/// A server's side of RFC 5802, and it checks the client's proof.
fn scram_server(wrong_signature: bool) -> Server {
    serve(move |mut stream, told, say| {
        blocking(&stream);
        let mut parser = Parser::default();
        open(&mut stream, &mut parser, "<mechanisms xmlns='urn:ietf:params:xml:ns:xmpp-sasl'><mechanism>PLAIN</mechanism><mechanism>SCRAM-SHA-1</mechanism></mechanisms>");
        let auth = read_stanza(&mut stream, &mut parser);
        assert_eq!(auth.attr("mechanism"), Some("SCRAM-SHA-1"));
        let first = String::from_utf8(BASE64.decode(auth.text.trim()).unwrap()).unwrap();
        let bare = first.strip_prefix("n,,").unwrap().to_owned();
        let nonce = bare.split(',').find_map(|part| part.strip_prefix("r=")).unwrap().to_owned();
        let salt = b"salty-salt-16byt";
        let server_first = format!("r={nonce}server,s={},i=4096", BASE64.encode(salt));
        stream.write_all(format!("<challenge xmlns='urn:ietf:params:xml:ns:xmpp-sasl'>{}</challenge>", BASE64.encode(&server_first)).as_bytes()).unwrap();
        let response = read_stanza(&mut stream, &mut parser);
        let client_final = String::from_utf8(BASE64.decode(response.text.trim()).unwrap()).unwrap();
        let (without_proof, proof) = client_final.rsplit_once(",p=").unwrap();
        let mut salted = [0u8; 20];
        pbkdf2::derive(pbkdf2::PBKDF2_HMAC_SHA1, NonZeroU32::new(4096).unwrap(), salt, b"pencil", &mut salted);
        let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, &salted);
        let client_key = hmac::sign(&key, b"Client Key");
        let stored = digest::digest(&digest::SHA1_FOR_LEGACY_USE_ONLY, client_key.as_ref());
        let message = format!("{bare},{server_first},{without_proof}");
        let signature = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, stored.as_ref()), message.as_bytes());
        let want: Vec<u8> = client_key.as_ref().iter().zip(signature.as_ref()).map(|(a, b)| a ^ b).collect();
        assert_eq!(BASE64.decode(proof).unwrap(), want);
        let server_key = hmac::sign(&key, b"Server Key");
        let verify = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, server_key.as_ref()), message.as_bytes());
        let text = if wrong_signature { "v=AAAA".to_owned() } else { format!("v={}", BASE64.encode(verify.as_ref())) };
        stream.write_all(format!("<success xmlns='urn:ietf:params:xml:ns:xmpp-sasl'>{}</success>", BASE64.encode(text)).as_bytes()).unwrap();
        if wrong_signature {
            thread::sleep(Duration::from_secs(1));
            return;
        }
        stream.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
        bind_and_play(stream, &mut parser, &told, &say);
    })
}

#[test]
fn scram_is_chosen_over_plain_and_proves_both_sides() {
    let server = scram_server(false);
    let _session = login(&settings(server.port, true)).unwrap();
    next_told(&server, "presence");
}

#[test]
fn a_server_that_cant_prove_the_password_is_refused() {
    let server = scram_server(true);
    let Err(failure) = login(&settings(server.port, true)) else { panic!("logged in") };
    assert!(failure.reason.contains("prove"), "{}", failure.reason);
}

fn tls_server(certificate: rcgen::CertifiedKey<rcgen::KeyPair>) -> Server {
    serve(move |mut stream, told, say| {
        blocking(&stream);
        let mut parser = Parser::default();
        open(&mut stream, &mut parser, "<starttls xmlns='urn:ietf:params:xml:ns:xmpp-tls'><required/></starttls>");
        read_stanza(&mut stream, &mut parser);
        stream.write_all(b"<proceed xmlns='urn:ietf:params:xml:ns:xmpp-tls'/>").unwrap();
        let chain = vec![certificate.cert.der().clone()];
        let key = rustls::pki_types::PrivateKeyDer::try_from(certificate.signing_key.serialize_der()).unwrap();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ServerConfig::builder_with_provider(provider).with_safe_default_protocol_versions().unwrap().with_no_client_auth().with_single_cert(chain, key).unwrap();
        let connection = rustls::ServerConnection::new(Arc::new(config)).unwrap();
        let mut tls = rustls::StreamOwned::new(connection, stream);
        parser.restart();
        let event = {
            let mut chunk = [0u8; 4096];
            loop {
                if let Some(event) = parser.next().unwrap() {
                    break event;
                }
                match tls.read(&mut chunk) {
                    Ok(0) | Err(_) => return,
                    Ok(count) => parser.feed(&chunk[..count]).unwrap(),
                }
            }
        };
        assert!(matches!(event, Event::Open(_)));
        tls.write_all(format!("{OPEN}<stream:features><mechanisms xmlns='urn:ietf:params:xml:ns:xmpp-sasl'><mechanism>PLAIN</mechanism></mechanisms></stream:features>").as_bytes()).unwrap();
        let auth = read_stanza(&mut tls, &mut parser);
        assert_eq!(BASE64.decode(auth.text.trim()).unwrap(), b"\0user\0pencil");
        tls.write_all(b"<success xmlns='urn:ietf:params:xml:ns:xmpp-sasl'/>").unwrap();
        tls.sock.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
        bind_and_play(tls, &mut parser, &told, &say);
    })
}

#[test]
fn starttls_upgrades_before_the_password_is_sent() {
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate.cert.der().clone()).unwrap();
    let server = tls_server(certificate);
    let mut wanted = settings(server.port, true);
    wanted.require_tls = true;
    wanted.roots = Some(Arc::new(roots));
    let mut session = login(&wanted).unwrap();
    assert!(session.secure);
    next_told(&server, "presence");
    server.say.send("<message from='a@b.c/x' type='chat'><body>over tls</body></message>".into()).unwrap();
    assert_eq!(poll_until(&mut session), vec![Incoming::Message { from: "a@b.c/x".into(), body: "over tls".into() }]);
}

#[test]
fn a_certificate_nobody_trusts_stops_the_login() {
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
    let server = tls_server(certificate);
    let wanted = settings(server.port, true);
    let Err(failure) = login(&wanted) else { panic!("trusted a stranger") };
    assert!(failure.reason.contains("TLS"), "{}", failure.reason);
    assert!(!failure.authenticated);
}
