//! The far end of `plugin-tools/tools/xmpp_live.py`: a person on a real XMPP server talking to a
//! real game server through EssentialsXMPP. Run by that script, not by `cargo test` alone.
use essentials_xmpp::client::{Incoming, Session, Settings, login};
use std::time::{Duration, Instant};

fn wait_for(session: &mut Session, wanted: &str) {
    let end = Instant::now() + Duration::from_secs(40);
    while Instant::now() < end {
        for Incoming::Message { body, .. } in session.poll().expect("the stream") {
            println!("HEARD {body}");
            if body.contains(wanted) {
                return;
            }
        }
    }
    panic!("never heard {wanted:?}");
}

#[test]
#[ignore = "run by xmpp_live.py"]
fn the_far_end() {
    let server = std::env::var("PLUGIN_ADDRESS").expect("the plugin's address");
    let mut session = login(&Settings {
        server: "127.0.0.1".into(),
        port: 15223,
        service: "localhost".into(),
        user: "partner".into(),
        password: "pencil".into(),
        sasl: true,
        require_tls: false,
        roots: None,
    })
    .expect("partner logs in");
    std::fs::write(std::env::var("READY_FILE").expect("a file"), "ready").unwrap();
    wait_for(&mut session, "Player Bot0 joined the game");
    wait_for(&mut session, "[Console] hello from the console");
    session.message(&server, "plain line from xmpp").unwrap();
    session.message(&server, "/say console line from xmpp").unwrap();
    session.message(&server, "@Bot0 a private line").unwrap();
    session.message(&server, "@nobody a lost line").unwrap();
    wait_for(&mut session, "User @nobody not found");
    std::thread::sleep(Duration::from_secs(1));
    session.close();
    println!("PARTNER DONE");
}
