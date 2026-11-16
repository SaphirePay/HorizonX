//! The Tebex plugin API (`https://plugin.tebex.io`): the requests the Java SDK makes, with the same
//! headers and bodies, and the answers read into plain structs. Runs on the HTTP threads only.

use crate::util;
use serde_json::{Value, json};
use std::time::Duration;

pub const BASE: &str = "https://plugin.tebex.io";

#[derive(Clone, Debug, Default)]
pub struct Store {
    pub id: i64,
    pub domain: String,
    pub name: String,
    pub currency_iso: String,
    pub currency_symbol: String,
    pub online_mode: bool,
    pub game_type: String,
}

#[derive(Clone, Debug, Default)]
pub struct ServerInfo {
    pub store: Store,
    pub server_id: i64,
    pub server_name: String,
}

#[derive(Clone, Debug, Default)]
pub struct Package {
    pub id: i64,
    pub order: i64,
    pub name: String,
    pub price: f64,
    pub item: String,
    pub sale: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct SubCategory {
    pub id: i64,
    pub order: i64,
    pub name: String,
    pub item: String,
    pub packages: Vec<Package>,
}

#[derive(Clone, Debug, Default)]
pub struct Category {
    pub id: i64,
    pub order: i64,
    pub name: String,
    pub item: String,
    pub packages: Vec<Package>,
    pub subs: Vec<SubCategory>,
}

/// `QueuedPlayer`: who a purchase is for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QueuedPlayer {
    pub id: i64,
    pub name: String,
    /// Dashed, or none (a Bedrock id, or none given).
    pub uuid: Option<String>,
    pub xuid: Option<String>,
}

impl QueuedPlayer {
    pub fn new(id: i64, name: &str, uuid: Option<&str>, xuid: Option<&str>) -> Self {
        let uuid = uuid.filter(|u| !util::is_blank(Some(u)) && !util::looks_like_xuid(Some(u))).map(util::dashed);
        let xuid = xuid.filter(|x| !util::is_blank(Some(x))).map(|x| x.trim().to_owned());
        Self { id, name: name.to_owned(), uuid, xuid }
    }
    pub fn from_json(v: &Value) -> Self {
        let mut uuid = v["uuid"].as_str().map(str::to_owned);
        let mut xuid = v["xuid"].as_str().map(str::to_owned);
        if xuid.is_none() && util::looks_like_xuid(uuid.as_deref()) {
            xuid = uuid.take();
        }
        Self::new(int(&v["id"]), v["name"].as_str().unwrap_or_default(), uuid.as_deref(), xuid.as_deref())
    }
    pub fn has_uuid(&self) -> bool {
        self.uuid.as_deref().is_some_and(|u| u != util::EMPTY_UUID)
    }
    /// `getDefaultCommandIdentifier` (no Floodgate here, so also `resolveCommandPlayerId`).
    pub fn command_id(&self) -> String {
        if self.has_uuid() { self.uuid.clone().unwrap_or_default() } else { self.name.clone() }
    }
}

#[derive(Clone, Debug)]
pub struct QueuedCommand {
    pub id: i64,
    pub command: String,
    pub payment: i64,
    pub delay: i64,
    pub slots: i64,
    pub player: QueuedPlayer,
}

impl QueuedCommand {
    fn from_json(v: &Value, player: QueuedPlayer) -> Self {
        let conditions = &v["conditions"];
        Self {
            id: int(&v["id"]),
            command: v["command"].as_str().unwrap_or_default().to_owned(),
            payment: int(&v["payment"]),
            delay: int(&conditions["delay"]),
            slots: int(&conditions["slots"]),
            player,
        }
    }
    /// `getParsedCommand`: `{username}`, `{name}`, `{id}`, `{uuid}`.
    pub fn parsed(&self) -> String {
        let id = self.player.command_id();
        self.command.replace("{username}", &self.player.name).replace("{name}", &self.player.name).replace("{id}", &id).replace("{uuid}", &id)
    }
}

pub struct DuePlayers {
    pub execute_offline: bool,
    pub next_check: i64,
    pub players: Vec<QueuedPlayer>,
}

pub struct Goal {
    pub name: String,
    pub current: f64,
    pub target: f64,
    pub status: String,
}

pub struct Lookup {
    pub username: String,
    pub id: String,
    pub chargeback_rate: i64,
    pub ban_count: i64,
    pub payments: usize,
}

/// A Java `int` from JSON: a number, a numeric string, or 0 (`null`).
fn int(v: &Value) -> i64 {
    v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)).or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).unwrap_or(0)
}

fn float(v: &Value) -> f64 {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).unwrap_or(0.0)
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn truthy(v: &Value) -> bool {
    v.as_bool().unwrap_or_else(|| int(v) != 0)
}

fn package(v: &Value) -> Package {
    let sale = &v["sale"];
    Package {
        id: int(&v["id"]),
        order: int(&v["order"]),
        name: text(&v["name"]),
        price: float(&v["price"]),
        item: text(&v["gui_item"]),
        sale: truthy(&sale["active"]).then(|| float(&sale["discount"])),
    }
}

fn packages(v: &Value) -> Vec<Package> {
    let mut list: Vec<Package> = v.as_array().into_iter().flatten().map(package).collect();
    list.sort_by_key(|p| p.order);
    list
}

pub fn parse_listing(body: &Value) -> Vec<Category> {
    let mut cats: Vec<Category> = body["categories"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| Category {
            id: int(&c["id"]),
            order: int(&c["order"]),
            name: text(&c["name"]),
            item: text(&c["gui_item"]),
            packages: packages(&c["packages"]),
            subs: c["subcategories"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|s| SubCategory { id: int(&s["id"]), order: int(&s["order"]), name: text(&s["name"]), item: text(&s["gui_item"]), packages: packages(&s["packages"]) })
                .collect(),
        })
        .collect();
    cats.sort_by_key(|c| c.order);
    cats
}

pub fn parse_info(body: &Value) -> Option<ServerInfo> {
    let account = body.get("account")?;
    let server = body.get("server")?;
    Some(ServerInfo {
        store: Store {
            id: int(&account["id"]),
            domain: text(&account["domain"]),
            name: text(&account["name"]),
            currency_iso: text(&account["currency"]["iso_4217"]),
            currency_symbol: text(&account["currency"]["symbol"]),
            online_mode: truthy(&account["online_mode"]),
            game_type: text(&account["game_type"]),
        },
        server_id: int(&server["id"]),
        server_name: text(&server["name"]),
    })
}

/// What went wrong with a request, worded as the Java SDK words it.
#[derive(Debug, Clone)]
pub enum Fail {
    /// 404 (and 403 for `/information`): `ServerNotFoundException`.
    NotFound,
    Status(u16, String),
    Io(String),
}

impl std::fmt::Display for Fail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => f.write_str("Server not found"),
            Self::Status(code, msg) if !msg.is_empty() => write!(f, "{msg}"),
            Self::Status(code, _) => write!(f, "Unexpected status code ({code})"),
            Self::Io(msg) => f.write_str(msg),
        }
    }
}

impl Fail {
    pub fn code(&self) -> Option<u16> {
        match self {
            Self::Status(code, _) => Some(*code),
            Self::NotFound => Some(404),
            Self::Io(_) => None,
        }
    }
}

pub type Res<T> = Result<T, Fail>;

/// One HTTP client, kept alive between requests (OkHttp's pool), shared by the HTTP threads.
#[derive(Clone)]
pub struct Client {
    agent: ureq::Agent,
    pub user_agent: String,
}

pub struct Reply {
    pub code: u16,
    pub body: String,
}

impl Client {
    pub fn new(user_agent: String) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(10))
            .timeout_write(Duration::from_secs(10))
            .max_idle_connections_per_host(4)
            .build();
        Self { agent, user_agent }
    }

    /// Any answer with a status is a reply (4xx/5xx too); only a dead connection is an error.
    pub fn send(&self, method: &str, url: &str, secret: Option<&str>, body: Option<&Value>) -> Result<Reply, String> {
        let url = if url.starts_with("http") { url.to_owned() } else { format!("{BASE}{url}") };
        let mut req = self.agent.request(method, &url).set("User-Agent", &self.user_agent).set("Accept", "application/json");
        if let Some(secret) = secret {
            req = req.set("X-Tebex-Secret", secret);
        }
        if crate::verbose() {
            crate::post_log(crate::Level::Info, format!("[DEBUG]  -> {method} {url} | {}", body.map_or_else(|| "No body".to_owned(), Value::to_string)));
        }
        let answer = match body {
            Some(body) => req.set("Content-Type", "application/json; charset=utf-8").send_string(&body.to_string()),
            None => req.call(),
        };
        let reply = match answer {
            Ok(resp) => Reply { code: resp.status(), body: resp.into_string().unwrap_or_default() },
            Err(ureq::Error::Status(code, resp)) => Reply { code, body: resp.into_string().unwrap_or_default() },
            Err(ureq::Error::Transport(t)) => return Err(t.to_string()),
        };
        if crate::verbose() {
            crate::post_log(crate::Level::Info, format!("[DEBUG] {} <- {method} {url}  | {}", reply.code, reply.body));
        }
        Ok(reply)
    }

    fn get_json(&self, url: &str, secret: &str, ok: u16) -> Res<Value> {
        let reply = self.send("GET", url, Some(secret), None).map_err(Fail::Io)?;
        match reply.code {
            404 => Err(Fail::NotFound),
            c if c == ok => serde_json::from_str(&reply.body).map_err(|e| Fail::Io(format!("Unexpected response: {e}"))),
            c => Err(Fail::Status(c, error_message(&reply.body))),
        }
    }

    pub fn information(&self, secret: &str) -> Res<ServerInfo> {
        let reply = self.send("GET", "/information", Some(secret), None).map_err(Fail::Io)?;
        match reply.code {
            404 | 403 => Err(Fail::NotFound),
            200 => serde_json::from_str::<Value>(&reply.body).ok().as_ref().and_then(parse_info).ok_or_else(|| Fail::Io("Unexpected response when getting store information.".into())),
            c => Err(Fail::Status(c, String::new())),
        }
    }

    pub fn due_players(&self, secret: &str) -> Res<DuePlayers> {
        let v = self.get_json("/queue", secret, 200)?;
        Ok(DuePlayers {
            execute_offline: truthy(&v["meta"]["execute_offline"]),
            next_check: int(&v["meta"]["next_check"]),
            players: v["players"].as_array().into_iter().flatten().map(QueuedPlayer::from_json).collect(),
        })
    }

    pub fn online_commands(&self, secret: &str, player: &QueuedPlayer) -> Res<Vec<QueuedCommand>> {
        let v = self.get_json(&format!("/queue/online-commands/{}", player.id), secret, 200)?;
        Ok(v["commands"].as_array().into_iter().flatten().map(|c| QueuedCommand::from_json(c, player.clone())).collect())
    }

    pub fn offline_commands(&self, secret: &str) -> Res<Vec<QueuedCommand>> {
        let v = self.get_json("/queue/offline-commands", secret, 200)?;
        Ok(v["commands"].as_array().into_iter().flatten().map(|c| QueuedCommand::from_json(c, QueuedPlayer::from_json(&c["player"]))).collect())
    }

    /// One request for any number of done commands (the Java SDK sends one per command).
    pub fn delete_commands(&self, secret: &str, ids: &[i64]) -> Res<()> {
        let reply = self.send("DELETE", "/queue", Some(secret), Some(&json!({ "ids": ids }))).map_err(Fail::Io)?;
        match reply.code {
            404 => Err(Fail::NotFound),
            204 | 200 => Ok(()),
            c => Err(Fail::Status(c, String::new())),
        }
    }

    pub fn listing(&self, secret: &str) -> Res<Vec<Category>> {
        Ok(parse_listing(&self.get_json("/listing", secret, 200)?))
    }

    pub fn checkout(&self, secret: &str, package: i64, username: &str) -> Res<String> {
        let reply = self.send("POST", "/checkout", Some(secret), Some(&json!({ "package_id": package, "username": username }))).map_err(Fail::Io)?;
        match reply.code {
            404 => Err(Fail::NotFound),
            201 | 200 => serde_json::from_str::<Value>(&reply.body).ok().and_then(|v| v["url"].as_str().map(str::to_owned)).ok_or_else(|| Fail::Io(format!("Unexpected response when creating checkout url for package {package}."))),
            c => Err(Fail::Status(c, error_message(&reply.body))),
        }
    }

    pub fn ban(&self, secret: &str, user: &str, ip: &str, reason: &str) -> Res<bool> {
        let reply = self.send("POST", "/bans", Some(secret), Some(&json!({ "user": user, "ip": ip, "reason": reason }))).map_err(Fail::Io)?;
        Ok(reply.code == 200)
    }

    pub fn goals(&self, secret: &str) -> Res<Vec<Goal>> {
        let v = self.get_json("/community_goals", secret, 200)?;
        Ok(v.as_array()
            .into_iter()
            .flatten()
            .map(|g| Goal { name: text(&g["name"]), current: float(&g["current"]), target: float(&g["target"]), status: text(&g["status"]).to_ascii_uppercase() })
            .collect())
    }

    pub fn lookup(&self, secret: &str, username: &str) -> Res<Option<Lookup>> {
        let reply = self.send("GET", &format!("/user/{username}"), Some(secret), None).map_err(Fail::Io)?;
        if !matches!(reply.code, 200 | 404 | 400) {
            return Err(Fail::Status(reply.code, String::new()));
        }
        if reply.body.trim() == "[]" {
            return Ok(None);
        }
        let v: Value = serde_json::from_str(&reply.body).map_err(|e| Fail::Io(e.to_string()))?;
        if let Some(msg) = v["error_message"].as_str() {
            return Err(Fail::Io(msg.to_owned()));
        }
        Ok(Some(Lookup {
            username: text(&v["player"]["username"]),
            id: text(&v["player"]["id"]),
            chargeback_rate: int(&v["chargebackRate"]),
            ban_count: int(&v["banCount"]),
            payments: v["payments"].as_array().map_or(0, Vec::len),
        }))
    }

    /// `/events` and the plugin logs: batches of `size`, halved on 413, as the Java SDK does.
    pub fn post_batches(&self, url: &str, secret: Option<&str>, items: &[Value], mut size: usize) -> bool {
        let mut offset = 0;
        while offset < items.len() {
            let end = (offset + size).min(items.len());
            let reply = match self.send("POST", url, secret, Some(&Value::Array(items[offset..end].to_vec()))) {
                Ok(reply) => reply,
                Err(_) => return false,
            };
            match reply.code {
                413 => {
                    size /= 2;
                    if size < 1 {
                        return false;
                    }
                }
                c if (200..300).contains(&c) => offset = end,
                _ => return false,
            }
        }
        true
    }
}

/// A 400's `error_message`, when the API gives one.
fn error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body).ok().and_then(|v| v["error_message"].as_str().map(str::to_owned)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_is_sorted_and_read() {
        let body = json!({ "categories": [
            { "id": 2, "order": 2, "name": "Ranks", "gui_item": "DIAMOND", "packages": [
                { "id": 11, "order": 2, "name": "MVP", "price": 10.0, "image": "", "gui_item": "", "sale": { "active": true, "discount": 2.5 } },
                { "id": 10, "order": 1, "name": "VIP", "price": "5.00", "image": "", "gui_item": "35:14", "sale": { "active": false, "discount": 0 } } ],
              "subcategories": [ { "id": 3, "order": 0, "name": "Monthly", "gui_item": "", "packages": [] } ] },
            { "id": 1, "order": 1, "name": "Keys", "gui_item": "", "packages": [], "subcategories": [] } ] });
        let cats = parse_listing(&body);
        assert_eq!(cats[0].name, "Keys");
        assert_eq!(cats[1].packages[0].name, "VIP");
        assert_eq!(cats[1].packages[0].price, 5.0);
        assert_eq!(cats[1].packages[1].sale, Some(2.5));
        assert_eq!(cats[1].subs[0].name, "Monthly");
    }

    #[test]
    fn queued_players_and_commands() {
        let p = QueuedPlayer::from_json(&json!({ "id": 7, "name": "Steve", "uuid": "069a79f444e94726a5befca90e38aaf5" }));
        assert_eq!(p.uuid.as_deref(), Some("069a79f4-44e9-4726-a5be-fca90e38aaf5"));
        let bedrock = QueuedPlayer::from_json(&json!({ "id": 8, "name": "Bed", "uuid": "2535412345678901" }));
        assert_eq!(bedrock.uuid, None);
        assert_eq!(bedrock.xuid.as_deref(), Some("2535412345678901"));
        assert_eq!(bedrock.command_id(), "Bed");
        let c = QueuedCommand::from_json(&json!({ "id": 1, "command": "lp user {uuid} parent add vip {name}", "payment": null, "conditions": { "delay": 5, "slots": 2 } }), p);
        assert_eq!(c.parsed(), "lp user 069a79f4-44e9-4726-a5be-fca90e38aaf5 parent add vip Steve");
        assert_eq!((c.delay, c.slots, c.payment), (5, 2, 0));
    }

    #[test]
    fn info() {
        let v = json!({ "account": { "id": 5, "domain": "https://x.tebex.io", "name": "X", "currency": { "iso_4217": "USD", "symbol": "$" }, "online_mode": true, "game_type": "Minecraft (Java)", "log_events": false }, "server": { "id": 9, "name": "Main" } });
        let info = parse_info(&v).unwrap();
        assert_eq!((info.store.currency_symbol.as_str(), info.server_name.as_str(), info.server_id), ("$", "Main", 9));
    }
}
