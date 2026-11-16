//! `client` against a real server: `cargo test --test real_server -- --ignored` with Prosody on
//! 127.0.0.1:15222, accounts `ess` and `partner` on `localhost`, password `pencil`, and its
//! certificate in the file `XMPP_CERT` names. Not run by default: it needs that server.
use essentials_xmpp::client::{Incoming, Session, Settings, login};
use rustls::pki_types::CertificateDer;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn roots() -> Arc<rustls::RootCertStore> {
    let path = std::env::var("XMPP_CERT").expect("XMPP_CERT names the server's certificate");
    let text = std::fs::read_to_string(path).expect("the certificate");
    let body: String = text.lines().filter(|line| !line.starts_with("-----")).collect();
    let der = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, body).expect("PEM");
    let mut store = rustls::RootCertStore::empty();
    store.add(CertificateDer::from(der)).expect("a certificate");
    Arc::new(store)
}

fn settings(user: &str) -> Settings {
    Settings {
        server: "127.0.0.1".into(),
        port: 15222,
        service: "localhost".into(),
        user: user.into(),
        password: "pencil".into(),
        sasl: true,
        require_tls: true,
        roots: Some(roots()),
    }
}

fn hear(session: &mut Session) -> Option<Incoming> {
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        if let Some(heard) = session.poll().expect("the stream").into_iter().next() {
            return Some(heard);
        }
    }
    None
}

#[test]
#[ignore = "needs a real XMPP server"]
fn logs_in_over_tls_and_messages_cross_both_ways() {
    let mut ess = login(&settings("ess")).expect("ess logs in");
    let mut partner = login(&settings("partner")).expect("partner logs in");
    assert!(ess.secure && partner.secure);
    ess.presence("No one online.", true).unwrap();
    ess.message("partner@localhost", "Player Steve joined the game").unwrap();
    let Some(Incoming::Message { from, body }) = hear(&mut partner) else { panic!("partner heard nothing") };
    assert!(from.starts_with("ess@localhost"), "{from}");
    assert_eq!(body, "Player Steve joined the game");
    partner.message("ess@localhost", "hello & <everyone>").unwrap();
    let Some(Incoming::Message { from, body }) = hear(&mut ess) else { panic!("ess heard nothing") };
    assert!(from.starts_with("partner@localhost"), "{from}");
    assert_eq!(body, "hello & <everyone>");
    ess.close();
    partner.close();
}

#[test]
#[ignore = "needs a real XMPP server"]
fn a_wrong_password_is_refused_without_a_panic() {
    let mut wrong = settings("ess");
    wrong.password = "not-it".into();
    let Err(failure) = login(&wrong) else { panic!("logged in with the wrong password") };
    assert!(failure.connected && failure.secure && !failure.authenticated, "{failure:?}");
}

#[test]
#[ignore = "needs a real XMPP server"]
fn an_untrusted_certificate_stops_the_login() {
    let mut untrusted = settings("ess");
    untrusted.roots = None;
    let Err(failure) = login(&untrusted) else { panic!("trusted a self-signed certificate") };
    assert!(failure.reason.contains("TLS"), "{}", failure.reason);
}

#[test]
#[ignore = "needs a real XMPP server"]
fn the_old_login_fails_with_advice_on_a_modern_server() {
    let mut old = settings("ess");
    old.sasl = false;
    let Err(failure) = login(&old) else { panic!("the old login worked") };
    assert!(failure.reason.contains("sasl-enabled"), "{}", failure.reason);
}
