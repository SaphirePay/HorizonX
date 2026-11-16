//! Skins: a player's name or uuid (Mojang's profile and session servers), or an image URL (MineSkin), fetched on a
//! thread of their own and cached in memory and in `.data/skins.json` (a skin is fetched once per identifier).

use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Clone, Debug)]
pub struct Texture {
    pub value: String,
    pub signature: String,
}

static CACHE: Mutex<Option<HashMap<String, Texture>>> = Mutex::new(None);
static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn key(identifier: &str) -> String {
    identifier.trim().to_ascii_lowercase()
}

pub fn get(identifier: &str) -> Option<Texture> {
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|c| c.get(&key(identifier)).cloned())
}

pub fn remember(identifier: &str, value: &str, signature: &str) {
    if identifier.is_empty() {
        return;
    }
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(key(identifier), Texture { value: value.to_owned(), signature: signature.to_owned() });
}

fn file() -> Option<std::path::PathBuf> {
    crate::folder().map(|f| f.join(".data").join("skins.json"))
}

pub fn load() {
    let Some(path) = file() else { return };
    let Ok(text) = std::fs::read_to_string(path) else { return };
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&text) else { return };
    let mut held = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let c = held.get_or_insert_with(HashMap::new);
    for (k, v) in map {
        if let (Some(value), Some(sig)) = (v["value"].as_str(), v["signature"].as_str()) {
            c.insert(k, Texture { value: value.to_owned(), signature: sig.to_owned() });
        }
    }
}

fn save() {
    let Some(path) = file() else { return };
    let map: serde_json::Map<String, Value> = CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|c| c.iter().map(|(k, t)| (k.clone(), json!({ "value": t.value, "signature": t.signature }))).collect()).unwrap_or_default();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, Value::Object(map).to_string());
}

pub fn is_uuid(s: &str) -> bool {
    let hex: String = s.chars().filter(|c| *c != '-').collect();
    hex.len() == 32 && hex.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn is_name(s: &str) -> bool {
    (1..=16).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(10)).user_agent("FancyNpcs-Horizon").build()
}

fn session(agent: &ureq::Agent, uuid: &str) -> Result<Texture, String> {
    let id: String = uuid.chars().filter(|c| *c != '-').collect();
    let body: Value = agent.get(&format!("https://sessionserver.mojang.com/session/minecraft/profile/{id}?unsigned=false")).call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
    let p = body["properties"].as_array().and_then(|a| a.iter().find(|p| p["name"] == "textures")).ok_or("no textures")?;
    Ok(Texture { value: p["value"].as_str().unwrap_or_default().to_owned(), signature: p["signature"].as_str().unwrap_or_default().to_owned() })
}

/// Fetches the skin (blocking: call on a thread of its own).
fn fetch(identifier: &str, slim: bool, mineskin_key: &str) -> Result<Texture, String> {
    let agent = agent();
    if is_uuid(identifier) {
        return session(&agent, identifier);
    }
    if identifier.starts_with("http://") || identifier.starts_with("https://") {
        let mut req = agent.post("https://api.mineskin.org/v2/generate").set("Content-Type", "application/json");
        if !mineskin_key.is_empty() {
            req = req.set("Authorization", &format!("Bearer {mineskin_key}"));
        }
        let body: Value = req.send_json(json!({ "url": identifier, "variant": if slim { "slim" } else { "classic" }, "visibility": "unlisted" })).map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
        let t = &body["skin"]["texture"]["data"];
        let value = t["value"].as_str().ok_or("MineSkin gave no texture")?;
        return Ok(Texture { value: value.to_owned(), signature: t["signature"].as_str().unwrap_or_default().to_owned() });
    }
    if is_name(identifier) {
        let body: Value = agent.get(&format!("https://api.mojang.com/users/profiles/minecraft/{identifier}")).call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
        let id = body["id"].as_str().ok_or("no such player")?;
        return session(&agent, id);
    }
    Err("not a player name, uuid or url".into())
}

/// The skin now when cached, else fetched off the server thread; `done(result)` then runs on the server thread.
pub fn request(identifier: &str, slim: bool, done: impl FnOnce(Result<Texture, String>) + Send + 'static) {
    if let Some(t) = get(identifier) {
        done(Ok(t));
        return;
    }
    let id = identifier.to_owned();
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).push(key(&id));
    let mineskin = crate::cfg_str("mineskin_api_key", "");
    std::thread::Builder::new()
        .name("FancyNpcs skin".into())
        .spawn(move || {
            let result = fetch(&id, slim, &mineskin);
            if let Ok(t) = &result {
                remember(&id, &t.value, &t.signature);
                save();
            }
            PENDING.lock().unwrap_or_else(|e| e.into_inner()).retain(|k| *k != key(&id));
            crate::on_main(move || done(result));
        })
        .ok();
}

pub fn is_pending(identifier: &str) -> bool {
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).contains(&key(identifier))
}
