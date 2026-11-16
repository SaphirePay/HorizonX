//! `config/entities` and the serializers beside them: what `userdata/*.yml` holds beyond plain values.
use crate::config::{float_value, scalar_bool, scalar_f64, scalar_i64, scalar_string};
use horizon_bukkit::{Location, World, get_world};
use regex::Regex;
use serde_yaml::{Mapping, Value};

fn field<'a>(node: &'a Value, key: &str) -> Option<&'a Value> {
    node.as_mapping()?.get(key)
}

fn set_field(node: &mut Value, key: &str, value: Value) {
    if !node.is_mapping() {
        *node = Value::Mapping(Mapping::new());
    }
    if let Value::Mapping(map) = node {
        map.insert(Value::from(key), value);
    }
}

/// `LazyLocation`: stays text until asked, so a home in a missing world survives.
#[derive(Clone, Debug, PartialEq)]
pub struct LazyLocation {
    pub world: String,
    pub world_name: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
}

impl LazyLocation {
    /// `LocationTypeSerializer.deserialize`: `None` if it has no world.
    pub fn read(node: &Value) -> Option<Self> {
        let world = field(node, "world").and_then(scalar_string).filter(|world| !world.is_empty())?;
        let number = |key: &str| field(node, key).map_or(0.0, scalar_f64);
        Some(Self { world, world_name: field(node, "world-name").and_then(scalar_string).unwrap_or_default(), x: number("x"), y: number("y"), z: number("z"), yaw: number("yaw") as f32, pitch: number("pitch") as f32 })
    }
    /// `LocationTypeSerializer.serialize`.
    pub fn write_to(&self, node: &mut Value) {
        set_field(node, "world", Value::from(self.world.clone()));
        set_field(node, "world-name", Value::from(self.world_name.clone()));
        set_field(node, "x", Value::from(self.x));
        set_field(node, "y", Value::from(self.y));
        set_field(node, "z", Value::from(self.z));
        set_field(node, "yaw", float_value(self.yaw));
        set_field(node, "pitch", float_value(self.pitch));
    }
    /// `location()`: by the world's id, its name in that field, then `world-name`.
    pub fn location(&mut self) -> Option<Location> {
        if self.world.is_empty() {
            return None;
        }
        let world = world_by_uid(&self.world).or_else(|| get_world(&self.world)).or_else(|| if self.world_name.is_empty() { None } else { get_world(&self.world_name) })?;
        self.world = world.get_uid();
        world.get_name().clone_into(&mut self.world_name);
        Some(Location::new(world, self.x, self.y, self.z, self.yaw, self.pitch))
    }
    /// `fromLocation`.
    pub fn from_location(location: &Location) -> Self {
        let world = location.get_world();
        Self { world: world.get_uid(), world_name: world.get_name().to_owned(), x: location.get_x(), y: location.get_y(), z: location.get_z(), yaw: location.get_yaw(), pitch: location.get_pitch() }
    }
}

fn world_by_uid(id: &str) -> Option<World> {
    horizon_bukkit::get_worlds().into_iter().find(|world| world.get_uid().eq_ignore_ascii_case(id))
}

/// `java.util.regex.Pattern`, as far as `regex` goes: `matches()` is a whole match. Look-around and
/// back-references don't compile: such a pattern from a user's file is kept but never matches.
#[derive(Clone, Debug)]
pub struct JavaPattern {
    source: String,
    whole: Option<Regex>,
    partial: Option<Regex>,
}

impl JavaPattern {
    pub fn new(source: &str) -> Self {
        Self { source: source.to_owned(), whole: Regex::new(&format!("^(?:{source})$")).ok(), partial: Regex::new(source).ok() }
    }
    /// False if `regex` can't read it, which Java's would have.
    pub fn is_valid(&self) -> bool {
        self.whole.is_some()
    }
    pub fn source(&self) -> &str {
        &self.source
    }
    /// `matcher(text).matches()`.
    pub fn matches(&self, text: &str) -> bool {
        self.whole.as_ref().is_some_and(|regex| regex.is_match(text))
    }
    /// `matcher(text).find()`, what `asPredicate` means.
    pub fn find(&self, text: &str) -> bool {
        self.partial.as_ref().is_some_and(|regex| regex.is_match(text))
    }
}

impl PartialEq for JavaPattern {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
    }
}

/// `CommandCooldown`, with `CommandCooldownSerializer`.
#[derive(Clone, Debug, PartialEq)]
pub struct CommandCooldown {
    pub pattern: JavaPattern,
    pub value: i64,
}

impl CommandCooldown {
    pub fn read(node: &Value) -> Option<Self> {
        Some(Self { pattern: JavaPattern::new(&field(node, "pattern").and_then(scalar_string)?), value: scalar_i64(field(node, "value")?) })
    }
    pub fn write(&self) -> Value {
        let mut node = Value::Mapping(Mapping::new());
        set_field(&mut node, "pattern", Value::from(self.pattern.source()));
        set_field(&mut node, "value", Value::from(self.value));
        node
    }
}

/// `MailMessage`, with `MailMessageSerializer`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailMessage {
    pub read: bool,
    pub legacy: bool,
    pub sender_name: Option<String>,
    pub sender_uuid: Option<String>,
    pub timestamp: i64,
    pub expire: i64,
    pub message: String,
}

impl MailMessage {
    pub fn read(node: &Value) -> Option<Self> {
        let legacy = field(node, "legacy").is_none_or(scalar_bool);
        let long = |key: &str| if legacy { 0 } else { field(node, key).map_or(0, scalar_i64) };
        Some(Self {
            read: field(node, "read").is_some_and(scalar_bool),
            legacy,
            sender_name: if legacy { None } else { field(node, "sender-name").and_then(scalar_string) },
            sender_uuid: field(node, "sender-uuid").and_then(scalar_string).filter(|uuid| uuid != "null"),
            timestamp: long("timestamp"),
            expire: long("expire"),
            message: field(node, "message").and_then(scalar_string)?,
        })
    }
    pub fn write(&self) -> Value {
        let mut node = Value::Mapping(Mapping::new());
        set_field(&mut node, "legacy", Value::from(self.legacy));
        set_field(&mut node, "read", Value::from(self.read));
        set_field(&mut node, "message", Value::from(self.message.clone()));
        if !self.legacy {
            set_field(&mut node, "sender-name", self.sender_name.clone().map_or(Value::Null, Value::from));
            set_field(&mut node, "sender-uuid", Value::from(self.sender_uuid.clone().unwrap_or_else(|| "null".to_owned())));
            set_field(&mut node, "timestamp", Value::from(self.timestamp));
            set_field(&mut node, "expire", Value::from(self.expire));
        }
        node
    }
    /// `isExpired`.
    pub fn is_expired(&self) -> bool {
        self.expire != 0 && crate::now_ms() >= self.expire
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_home_from_java_reads_and_writes_back_the_same_fields() {
        let text = "world: 5f4e6a00-1111-2222-3333-444455556666\nworld-name: world\nx: 10.5\ny: 64.0\nz: -3.25\nyaw: 90.0\npitch: 12.5\n";
        let node: Value = serde_yaml::from_str(text).unwrap();
        let home = LazyLocation::read(&node).unwrap();
        let mut back = Value::Mapping(Mapping::new());
        home.write_to(&mut back);
        assert_eq!(serde_yaml::to_string(&back).unwrap(), text);
    }

    #[test]
    fn a_location_with_no_world_is_nothing() {
        assert!(LazyLocation::read(&serde_yaml::from_str("x: 1").unwrap()).is_none());
    }

    #[test]
    fn patterns_match_whole_like_java() {
        let pattern = JavaPattern::new("home( .*)?");
        assert!(pattern.matches("home bed") && pattern.matches("home") && !pattern.matches("homes"));
    }

    #[test]
    fn a_mail_keeps_null_as_text() {
        let mail = MailMessage { read: false, legacy: false, sender_name: Some("a".into()), sender_uuid: None, timestamp: 1, expire: 0, message: "m".into() };
        let node = mail.write();
        assert_eq!(field(&node, "sender-uuid"), Some(&Value::from("null")));
        assert_eq!(MailMessage::read(&node).unwrap(), mail);
    }
}
