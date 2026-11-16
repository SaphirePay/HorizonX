//! Records what a server sends a client in the configuration stage that a world needs (enabled features, the
//! known packs, every registry, the tags), for horizon-proxy's queue world to send its waiting players. The
//! server must be offline mode and the same game version as the proxy's servers; the client is told it knows
//! the same packs the server offers, as a real game does, so the registries come without their data.
//!
//!     cargo run -p horizon-protocol --example limbo_capture -- 127.0.0.1:25797 limbo-config.bin
//!
//! The file: one record per packet, a u32 (little-endian) length and the packet body (its id and fields).
use horizon_protocol::{
    PROTOCOL_VERSION,
    connection::Connection,
    packets::{Empty, configuration as cfg, handshake, login},
    servers::parse_address,
};
use std::time::Duration;

fn varint(mut v: u32, out: &mut Vec<u8>) {
    loop {
        if v & !0x7F == 0 {
            out.push(v as u8);
            return;
        }
        out.push((v as u8 & 0x7F) | 0x80);
        v >>= 7;
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let (host, port) = parse_address(args.get(1).map_or("127.0.0.1", String::as_str)).ok_or("bad address")?;
    let file = args.get(2).map_or("limbo-config.bin", String::as_str);
    let mut c = Connection::connect(&host, port, Duration::from_secs(5))?;
    c.send(&handshake::Serverbound::Intention(handshake::Intention { protocol: PROTOCOL_VERSION, host: host.clone(), port, intent: handshake::LOGIN }).encode())?;
    let name = "LimboCapture";
    c.send(&login::Serverbound::Hello(login::ServerHello { name: name.into(), uuid: horizon_protocol::crypto::offline_uuid(name) }).encode())?;
    loop {
        let (id, payload) = c.recv()?;
        match login::Clientbound::decode(id, &payload)? {
            login::Clientbound::Compression(p) => c.set_compression(p.threshold),
            login::Clientbound::LoginFinished(_) => {
                c.send(&login::Serverbound::LoginAcknowledged(Empty).encode())?;
                break;
            }
            login::Clientbound::Disconnect(p) => return Err(p.reason_json.into()),
            _ => {}
        }
    }
    c.send(&cfg::Serverbound::ClientInformation(cfg::ClientInformation::new(8)).encode())?;
    let mut out = Vec::new();
    let mut keep = |id: i32, payload: &[u8], what: &str| {
        let mut body = Vec::new();
        varint(id as u32, &mut body);
        body.extend_from_slice(payload);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        println!("{what} ({} bytes)", body.len());
    };
    loop {
        let (id, payload) = c.recv()?;
        match cfg::Clientbound::decode(id, &payload) {
            Ok(cfg::Clientbound::UpdateEnabledFeatures(p)) => keep(id, &payload, &format!("features {:?}", p.features)),
            Ok(cfg::Clientbound::SelectKnownPacks(p)) => {
                keep(id, &payload, &format!("known packs {:?}", p.packs));
                c.send(&cfg::Serverbound::SelectKnownPacks(cfg::SelectKnownPacks { packs: p.packs }).encode())?;
            }
            Ok(cfg::Clientbound::RegistryData(p)) => keep(id, &payload, &format!("registry {} ({} entries)", p.registry, p.entries.len())),
            Ok(cfg::Clientbound::UpdateTags(p)) => keep(id, &payload, &format!("tags for {} registries", p.registries.len())),
            Ok(cfg::Clientbound::KeepAlive(p)) => c.send(&cfg::Serverbound::KeepAlive(p).encode())?,
            Ok(cfg::Clientbound::Ping(p)) => c.send(&cfg::Serverbound::Pong(p).encode())?,
            // A server that requires its pack waits for an answer: "loaded".
            Ok(cfg::Clientbound::ResourcePackPush(p)) => c.send(&cfg::Serverbound::ResourcePack(cfg::ResourcePackResponse { id: p.id, action: 0 }).encode())?,
            Ok(cfg::Clientbound::CodeOfConduct(_)) => c.send(&cfg::Serverbound::AcceptCodeOfConduct(Empty).encode())?,
            Ok(cfg::Clientbound::FinishConfiguration(_)) => break,
            Ok(cfg::Clientbound::Disconnect(p)) => return Err(format!("{:?}", p.reason).into()),
            _ => {}
        }
    }
    std::fs::write(file, &out)?;
    println!("wrote {file}: {} bytes", out.len());
    Ok(())
}
