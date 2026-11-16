//! The queue world, 2b2t style (owner 11-08): a waiting player stands in the End's empty sky, in spectator, with
//! "Position in queue: N" across the screen, the wait above the hotbar's place and the odd reminder in chat.
//!
//! The configuration stage a world needs (enabled features, known packs, registries, tags) is replayed from a file
//! recorded once from one of the network's servers (`cargo run -p horizon-protocol --example limbo_capture`).
//! The player is put above the End's build height, so the game opens the world at once without any chunk, and
//! they cannot fall or move off (an unloaded chunk holds a player still).

use horizon_protocol::packets::{configuration as cfg, play};
use serde_json::{Value, json};

/// One recorded configuration packet: its body (id and fields), and what it is.
pub struct Record {
    pub body: Vec<u8>,
    pub known_packs: bool,
}

pub struct Limbo {
    pub records: Vec<Record>,
    /// The End's place in the dimension_type registry, as the records list it.
    pub end_type: i32,
}

fn read_varint(b: &[u8]) -> Option<(i32, usize)> {
    let mut v: u32 = 0;
    for (i, byte) in b.iter().enumerate().take(5) {
        v |= u32::from(byte & 0x7F) << (7 * i);
        if byte & 0x80 == 0 {
            return Some((v as i32, i + 1));
        }
    }
    None
}

impl Limbo {
    /// Reads the recording; None (with the reason logged) when it is missing or not a usable recording.
    pub fn load(path: &std::path::Path) -> Option<Limbo> {
        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("queue world: no recording at {} ({e}); the queue uses its screen instead", path.display());
                return None;
            }
        };
        let mut records = Vec::new();
        let mut end_type = None;
        let mut at = 0;
        while at + 4 <= data.len() {
            let len = u32::from_le_bytes(data[at..at + 4].try_into().ok()?) as usize;
            let body = data.get(at + 4..at + 4 + len)?.to_vec();
            at += 4 + len;
            let (id, n) = read_varint(&body)?;
            let packet = cfg::Clientbound::decode(id, &body[n..]).ok()?;
            if let cfg::Clientbound::RegistryData(r) = &packet
                && r.registry == "minecraft:dimension_type"
            {
                end_type = r.entries.iter().position(|e| e.id == "minecraft:the_end").map(|i| i as i32);
            }
            records.push(Record { body, known_packs: matches!(packet, cfg::Clientbound::SelectKnownPacks(_)) });
        }
        let Some(end_type) = end_type else {
            eprintln!("queue world: {} has no End dimension type; the queue uses its screen instead", path.display());
            return None;
        };
        println!("queue world: {} configuration packets from {}", records.len(), path.display());
        Some(Limbo { records, end_type })
    }

    /// The play stage's opening: the world (the End), spectator, the chunks wait, the spot high above the world.
    pub fn join_packets(&self) -> Vec<Vec<u8>> {
        let spawn = play::SpawnInfo {
            dimension_type: self.end_type,
            dimension: "minecraft:the_end".into(),
            seed: 0,
            game_type: 3,
            previous_game_type: None,
            debug: false,
            flat: false,
            last_death: None,
            portal_cooldown: 0,
            sea_level: 63,
        };
        vec![
            play::Clientbound::Login(play::Login {
                player_id: 1,
                hardcore: false,
                levels: vec!["minecraft:the_end".into()],
                max_players: 1,
                chunk_radius: 2,
                simulation_distance: 2,
                reduced_debug_info: true,
                show_death_screen: false,
                limited_crafting: false,
                spawn,
                online_mode: false,
                enforces_secure_chat: false,
            })
            .encode(),
            // LEVEL_CHUNKS_LOAD_START: the game starts waiting for the world (which is ready at once up here).
            play::Clientbound::GameEvent(play::GameEvent { event: 13, param: 0.0 }).encode(),
            play::Clientbound::PlayerPosition(play::PlayerPosition { teleport: 1, position: [0.5, 1000.0, 0.5], delta: [0.0; 3], yaw: 0.0, pitch: 0.0, relatives: 0 }).encode(),
            // Titles: no fade, held between refreshes.
            play::Clientbound::SetTitlesAnimation(play::TitlesAnimation { fade_in: 0, stay: 200, fade_out: 20 }).encode(),
        ]
    }
}

const GOLD: &str = "#fbbf24";
const SOFT: &str = "#fcd34d";

fn text(v: Value) -> horizon_world::nbt::Tag {
    crate::screen::tag(&v)
}

/// The screen's lines for a standing: "Position in queue: N" (subtitle size) and the wait (the action bar).
pub fn screen_packets(position: usize, wait: &str, tier: Option<(&str, &str)>, held: Option<&str>) -> Vec<Vec<u8>> {
    let subtitle = json!({ "text": "", "extra": [
        { "text": "Position in queue: ", "color": GOLD },
        { "text": crate::screen::thousands(position), "color": GOLD, "bold": true },
    ]});
    let mut bar = vec![];
    if let Some((name, color)) = tier {
        bar.push(json!({ "text": format!("◆ {name}  "), "color": color, "bold": true }));
    }
    match held {
        Some(why) => bar.push(json!({ "text": why, "color": SOFT })),
        None => {
            let label = if tier.is_some() { "Estimated wait: " } else { "Non-Priority Queue Estimated Wait: " };
            bar.push(json!({ "text": label, "color": SOFT }));
            bar.push(json!({ "text": wait, "color": SOFT, "bold": true }));
        }
    }
    vec![
        play::Clientbound::SetSubtitleText(play::Text { text: text(subtitle) }).encode(),
        play::Clientbound::SetTitleText(play::Text { text: text(json!({ "text": "" })) }).encode(),
        play::Clientbound::SystemChat(play::SystemChat { content: text(json!({ "text": "", "extra": bar })), overlay: true }).encode(),
    ]
}

/// A chat line.
pub fn chat(v: Value) -> Vec<u8> {
    play::Clientbound::SystemChat(play::SystemChat { content: text(v), overlay: false }).encode()
}

/// "Position in queue: N" in chat.
pub fn chat_position(position: usize) -> Vec<u8> {
    chat(json!({ "text": "", "extra": [
        { "text": "Position in queue: ", "color": GOLD },
        { "text": crate::screen::thousands(position), "color": GOLD, "bold": true },
    ]}))
}

/// The store reminder, the link clickable (none without a store).
pub fn chat_store(store: &str) -> Option<Vec<u8>> {
    if store.is_empty() {
        return None;
    }
    let shown = store.trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/');
    Some(chat(json!({ "text": "", "extra": [
        { "text": "You can purchase priority queue status to join the server faster, visit ", "color": GOLD },
        { "text": shown, "color": GOLD, "bold": true, "underlined": true, "click_event": { "action": "open_url", "url": store } },
    ]})))
}

/// The commands that leave the queue world for the hub.
pub fn is_leave(command: &str) -> bool {
    let word = command.trim().trim_start_matches('/').split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    matches!(word.as_str(), "hub" | "lobby" | "l" | "leave" | "leavequeue" | "lq" | "spawn")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leave_words() {
        assert!(is_leave("hub"));
        assert!(is_leave("/lq"));
        assert!(is_leave("LeaveQueue now"));
        assert!(!is_leave("msg bob hi"));
    }

    #[test]
    fn the_recording_loads_and_finds_the_end() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("limbo-config.bin");
        if let Some(l) = Limbo::load(&path) {
            assert!(l.records.iter().any(|r| r.known_packs));
            assert!(l.end_type >= 0);
            assert_eq!(l.join_packets().len(), 4);
        }
    }
}
