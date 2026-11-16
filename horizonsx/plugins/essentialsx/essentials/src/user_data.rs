//! `UserConfigHolder`: what `userdata/<uuid>.yml` holds, by Configurate's names (fields as
//! lower-case-dashed), with `@DeleteOnEmpty` fields gone when empty. Unknown keys, like `info`, stay.
use crate::big_decimal::BigDecimal;
use crate::config::{entries, scalar_bool, scalar_decimal, scalar_i64, scalar_string};
use crate::config_entities::{CommandCooldown, LazyLocation, MailMessage};
use serde_yaml::{Mapping, Sequence, Value};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Timestamps {
    pub last_teleport: i64,
    pub last_heal: i64,
    pub mute: i64,
    pub jail: i64,
    pub online_jail: i64,
    pub logout: i64,
    pub login: i64,
    pub kits: Vec<(String, i64)>,
    pub command_cooldowns: Vec<CommandCooldown>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UserConfigHolder {
    pub money: Option<BigDecimal>,
    /// In file order: Configurate reads a map into a `LinkedHashMap`.
    pub homes: Vec<(String, LazyLocation)>,
    pub nickname: Option<String>,
    /// Material names, as `MaterialTypeSerializer` writes them.
    pub unlimited: Vec<String>,
    pub powertools: Vec<(String, Vec<String>)>,
    pub last_location: Option<LazyLocation>,
    pub logout_location: Option<LazyLocation>,
    pub jail: Option<String>,
    pub mail: Vec<MailMessage>,
    pub teleport_enabled: bool,
    pub teleport_auto: bool,
    pub ignore: Vec<String>,
    pub god_mode: bool,
    pub fly_mode: bool,
    pub muted: bool,
    pub mute_reason: Option<String>,
    pub jailed: bool,
    pub ip_address: String,
    pub afk: bool,
    pub geolocation: Option<String>,
    pub social_spy: bool,
    pub npc: bool,
    pub last_account_name: Option<String>,
    pub npc_name: Option<String>,
    pub power_tools_enabled: bool,
    pub accepting_pay: bool,
    pub confirm_pay: Option<bool>,
    pub confirm_clear: Option<bool>,
    pub last_message_reply_recipient: Option<bool>,
    pub baltop_exempt: bool,
    pub shouting: Option<bool>,
    pub past_usernames: Vec<String>,
    pub timestamps: Timestamps,
}

impl Default for UserConfigHolder {
    fn default() -> Self {
        Self {
            money: None,
            homes: Vec::new(),
            nickname: None,
            unlimited: Vec::new(),
            powertools: Vec::new(),
            last_location: None,
            logout_location: None,
            jail: None,
            mail: Vec::new(),
            teleport_enabled: true,
            teleport_auto: false,
            ignore: Vec::new(),
            god_mode: false,
            fly_mode: false,
            muted: false,
            mute_reason: None,
            jailed: false,
            ip_address: String::new(),
            afk: false,
            geolocation: None,
            social_spy: false,
            npc: false,
            last_account_name: None,
            npc_name: None,
            power_tools_enabled: true,
            accepting_pay: true,
            confirm_pay: None,
            confirm_clear: None,
            last_message_reply_recipient: None,
            baltop_exempt: false,
            shouting: None,
            past_usernames: Vec::new(),
            timestamps: Timestamps::default(),
        }
    }
}

fn at<'a>(root: &'a Value, key: &str) -> Option<&'a Value> {
    root.as_mapping()?.get(key).filter(|value| !value.is_null())
}

fn text(root: &Value, key: &str) -> Option<String> {
    at(root, key).and_then(scalar_string)
}

fn flag(root: &Value, key: &str, default: bool) -> bool {
    at(root, key).map_or(default, scalar_bool)
}

fn opt_flag(root: &Value, key: &str) -> Option<bool> {
    at(root, key).map(scalar_bool)
}

fn long(root: &Value, key: &str) -> i64 {
    at(root, key).map_or(0, scalar_i64)
}

fn strings(node: Option<&Value>) -> Vec<String> {
    match node {
        Some(Value::Sequence(values)) => values.iter().filter_map(scalar_string).collect(),
        Some(value) => scalar_string(value).into_iter().collect(),
        None => Vec::new(),
    }
}

/// `Set<Material>`: `MaterialTypeSerializer` runs `matchMaterial`, which takes either spelling.
fn material_name(text: &str) -> String {
    text.trim_start_matches("minecraft:").to_ascii_uppercase()
}

impl UserConfigHolder {
    /// `getRootNode().get(UserConfigHolder.class)`.
    pub fn read(root: &Value) -> Self {
        let stamps = at(root, "timestamps").cloned().unwrap_or(Value::Null);
        Self {
            money: at(root, "money").and_then(scalar_decimal),
            homes: entries(at(root, "homes")).into_iter().filter_map(|(name, node)| Some((name, LazyLocation::read(&node)?))).collect(),
            nickname: text(root, "nickname"),
            unlimited: strings(at(root, "unlimited")).iter().map(|name| material_name(name)).collect(),
            powertools: entries(at(root, "powertools")).into_iter().map(|(item, commands)| (item, strings(Some(&commands)))).collect(),
            last_location: at(root, "lastlocation").and_then(LazyLocation::read),
            logout_location: at(root, "logoutlocation").and_then(LazyLocation::read),
            jail: text(root, "jail"),
            mail: at(root, "mail").and_then(Value::as_sequence).into_iter().flatten().filter_map(MailMessage::read).collect(),
            teleport_enabled: flag(root, "teleportenabled", true),
            teleport_auto: flag(root, "teleportauto", false),
            ignore: strings(at(root, "ignore")),
            god_mode: flag(root, "godmode", false),
            fly_mode: flag(root, "flymode", false),
            muted: flag(root, "muted", false),
            mute_reason: text(root, "mute-reason"),
            jailed: flag(root, "jailed", false),
            ip_address: text(root, "ip-address").unwrap_or_default(),
            afk: flag(root, "afk", false),
            geolocation: text(root, "geolocation"),
            social_spy: flag(root, "socialspy", false),
            npc: flag(root, "npc", false),
            last_account_name: text(root, "last-account-name"),
            npc_name: text(root, "npc-name"),
            power_tools_enabled: flag(root, "powertoolsenabled", true),
            accepting_pay: flag(root, "accepting-pay", true),
            confirm_pay: opt_flag(root, "confirm-pay"),
            confirm_clear: opt_flag(root, "confirm-clear"),
            last_message_reply_recipient: opt_flag(root, "last-message-reply-recipient"),
            baltop_exempt: flag(root, "baltop-exempt", false),
            shouting: opt_flag(root, "shouting"),
            past_usernames: strings(at(root, "past-usernames")),
            timestamps: Timestamps {
                last_teleport: long(&stamps, "lastteleport"),
                last_heal: long(&stamps, "lastheal"),
                mute: long(&stamps, "mute"),
                jail: long(&stamps, "jail"),
                online_jail: long(&stamps, "onlinejail"),
                logout: long(&stamps, "logout"),
                login: long(&stamps, "login"),
                kits: entries(at(&stamps, "kits")).into_iter().map(|(name, value)| (name, scalar_i64(&value))).collect(),
                command_cooldowns: at(&stamps, "command-cooldowns").and_then(Value::as_sequence).into_iter().flatten().filter_map(CommandCooldown::read).collect(),
            },
        }
    }

    /// `setRootHolder`: each field in its place, or gone.
    pub fn write(&self, root: &mut Value) {
        if !root.is_mapping() {
            *root = Value::Mapping(Mapping::new());
        }
        let Value::Mapping(map) = root else {
            unreachable!("made a mapping above");
        };
        let mut put = |key: &str, value: Option<Value>| match value {
            Some(value) => {
                map.insert(Value::from(key), value);
            }
            None => {
                map.shift_remove(key);
            }
        };
        put("money", self.money.as_ref().map(|money| Value::from(money.to_string())));
        put("homes", (!self.homes.is_empty()).then(|| locations(&self.homes)));
        put("nickname", self.nickname.clone().map(Value::from));
        put("unlimited", list(&self.unlimited));
        put("powertools", (!self.powertools.is_empty()).then(|| Value::Mapping(self.powertools.iter().map(|(item, commands)| (Value::from(item.clone()), Value::Sequence(commands.iter().cloned().map(Value::from).collect()))).collect())));
        put("lastlocation", self.last_location.as_ref().map(location));
        put("logoutlocation", self.logout_location.as_ref().map(location));
        put("jail", self.jail.clone().map(Value::from));
        put("mail", (!self.mail.is_empty()).then(|| Value::Sequence(self.mail.iter().map(MailMessage::write).collect())));
        put("teleportenabled", Some(Value::from(self.teleport_enabled)));
        put("teleportauto", Some(Value::from(self.teleport_auto)));
        put("ignore", list(&self.ignore));
        put("godmode", Some(Value::from(self.god_mode)));
        put("flymode", Some(Value::from(self.fly_mode)));
        put("muted", Some(Value::from(self.muted)));
        put("mute-reason", self.mute_reason.clone().map(Value::from));
        put("jailed", Some(Value::from(self.jailed)));
        put("ip-address", Some(Value::from(self.ip_address.clone())));
        put("afk", Some(Value::from(self.afk)));
        put("geolocation", self.geolocation.clone().filter(|text| !text.is_empty()).map(Value::from));
        put("socialspy", Some(Value::from(self.social_spy)));
        put("npc", Some(Value::from(self.npc)));
        put("last-account-name", self.last_account_name.clone().map(Value::from));
        put("npc-name", self.npc_name.clone().map(Value::from));
        put("powertoolsenabled", Some(Value::from(self.power_tools_enabled)));
        put("accepting-pay", Some(Value::from(self.accepting_pay)));
        put("confirm-pay", self.confirm_pay.map(Value::from));
        put("confirm-clear", self.confirm_clear.map(Value::from));
        put("last-message-reply-recipient", self.last_message_reply_recipient.map(Value::from));
        put("baltop-exempt", Some(Value::from(self.baltop_exempt)));
        put("shouting", self.shouting.map(Value::from));
        put("past-usernames", list(&self.past_usernames));
        put("timestamps", Some(self.timestamps.write()));
    }
}

impl Timestamps {
    fn write(&self) -> Value {
        let mut map = Mapping::new();
        map.insert(Value::from("lastteleport"), Value::from(self.last_teleport));
        map.insert(Value::from("lastheal"), Value::from(self.last_heal));
        map.insert(Value::from("mute"), Value::from(self.mute));
        map.insert(Value::from("jail"), Value::from(self.jail));
        map.insert(Value::from("onlinejail"), Value::from(self.online_jail));
        map.insert(Value::from("logout"), Value::from(self.logout));
        map.insert(Value::from("login"), Value::from(self.login));
        if !self.kits.is_empty() {
            map.insert(Value::from("kits"), Value::Mapping(self.kits.iter().map(|(name, time)| (Value::from(name.clone()), Value::from(*time))).collect()));
        }
        if !self.command_cooldowns.is_empty() {
            map.insert(Value::from("command-cooldowns"), Value::Sequence(self.command_cooldowns.iter().map(CommandCooldown::write).collect()));
        }
        Value::Mapping(map)
    }
}

fn list(values: &[String]) -> Option<Value> {
    (!values.is_empty()).then(|| Value::Sequence(values.iter().cloned().map(Value::from).collect::<Sequence>()))
}

fn location(place: &LazyLocation) -> Value {
    let mut node = Value::Mapping(Mapping::new());
    place.write_to(&mut node);
    node
}

fn locations(places: &[(String, LazyLocation)]) -> Value {
    Value::Mapping(places.iter().map(|(name, place)| (Value::from(name.clone()), location(place))).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const JAVA_FILE: &str = "money: '1500.25'\nhomes:\n  home:\n    world: 5f4e6a00-1111-2222-3333-444455556666\n    world-name: world\n    x: 1.5\n    y: 64.0\n    z: 2.5\n    yaw: 0.0\n    pitch: 0.0\nnickname: Bob\nteleportenabled: true\nteleportauto: false\ngodmode: false\nflymode: true\nmuted: false\njailed: false\nip-address: 127.0.0.1\nafk: false\nsocialspy: false\nnpc: false\nlast-account-name: Bob\npowertoolsenabled: true\naccepting-pay: true\nbaltop-exempt: false\ntimestamps:\n  lastteleport: 5\n  lastheal: 0\n  mute: 0\n  jail: 0\n  onlinejail: 0\n  logout: 10\n  login: 9\n  kits:\n    tools: 99\ninfo:\n  custom: kept\n";

    #[test]
    fn a_file_from_java_reads_right() {
        let holder = UserConfigHolder::read(&serde_yaml::from_str(JAVA_FILE).unwrap());
        assert_eq!(holder.money.as_ref().map(ToString::to_string).as_deref(), Some("1500.25"));
        assert_eq!(holder.homes[0].0, "home");
        assert_eq!(holder.nickname.as_deref(), Some("Bob"));
        assert!(holder.fly_mode);
        assert_eq!(holder.timestamps.kits, [("tools".to_owned(), 99)]);
    }

    #[test]
    fn writing_back_changes_nothing() {
        let mut root: Value = serde_yaml::from_str(JAVA_FILE).unwrap();
        UserConfigHolder::read(&root).write(&mut root);
        assert_eq!(serde_yaml::to_string(&root).unwrap(), JAVA_FILE);
    }

    #[test]
    fn a_number_for_money_reads_too() {
        assert_eq!(UserConfigHolder::read(&serde_yaml::from_str("money: 100").unwrap()).money, BigDecimal::parse("100"));
    }

    #[test]
    fn empty_lists_leave_the_file() {
        let mut root: Value = serde_yaml::from_str("homes: {}\nmail: []\n").unwrap();
        UserConfigHolder::read(&root).write(&mut root);
        let map = root.as_mapping().unwrap();
        assert!(map.get("homes").is_none() && map.get("mail").is_none());
    }
}
