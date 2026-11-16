//! What's needed of the MaxMind DB format to read GeoLite2: an address down the search tree to a
//! record, and a record's fields.
use std::collections::BTreeMap;

const METADATA_MARKER: &[u8] = b"\xab\xcd\xefMaxMind.com";

#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    Text(String),
    Number(u64),
    List(Vec<Field>),
    Map(BTreeMap<String, Field>),
    Other,
}

impl Field {
    pub fn get(&self, key: &str) -> Option<&Field> {
        if let Self::Map(map) = self { map.get(key) } else { None }
    }
    fn number(&self) -> u64 {
        if let Self::Number(value) = self { *value } else { 0 }
    }
}

pub struct Database {
    data: Vec<u8>,
    node_count: u64,
    record_size: u64,
    ip_version: u64,
    tree_size: usize,
}

impl Database {
    pub fn open(data: Vec<u8>) -> Option<Self> {
        let at = data.windows(METADATA_MARKER.len()).rposition(|window| window == METADATA_MARKER)? + METADATA_MARKER.len();
        let (metadata, _) = decode(&data, at, at)?;
        let node_count = metadata.get("node_count")?.number();
        let record_size = metadata.get("record_size")?.number();
        let ip_version = metadata.get("ip_version")?.number();
        let tree_size = usize::try_from(node_count * record_size / 4).ok()?;
        Some(Self { data, node_count, record_size, ip_version, tree_size })
    }
    fn record(&self, node: u64, right: bool) -> Option<u64> {
        let bytes = (self.record_size / 4) as usize;
        let start = usize::try_from(node).ok()? * bytes;
        let read = |from: usize, count: usize| self.data.get(from..from + count).map(|slice| slice.iter().fold(0u64, |sum, byte| (sum << 8) | u64::from(*byte)));
        match self.record_size {
            24 | 32 => read(start + if right { bytes / 2 } else { 0 }, bytes / 2),
            28 => {
                let middle = u64::from(*self.data.get(start + 3)?);
                let high = if right { middle & 0x0f } else { middle >> 4 };
                let low = read(start + if right { 4 } else { 0 }, 3)?;
                Some((high << 24) | low)
            }
            _ => None,
        }
    }
    /// The record for an address, `None` if the tree has none.
    pub fn lookup(&self, address: std::net::IpAddr) -> Option<Field> {
        let bytes: Vec<u8> = match address {
            std::net::IpAddr::V4(four) => four.octets().to_vec(),
            std::net::IpAddr::V6(six) => six.octets().to_vec(),
        };
        let mut node = 0;
        if self.ip_version == 6 && bytes.len() == 4 {
            for _ in 0..96 {
                node = self.record(node, false)?;
                if node >= self.node_count {
                    break;
                }
            }
        }
        for index in 0..bytes.len() * 8 {
            if node >= self.node_count {
                break;
            }
            let bit = bytes[index / 8] >> (7 - index % 8) & 1;
            node = self.record(node, bit == 1)?;
        }
        if node <= self.node_count {
            return None;
        }
        let base = self.tree_size + 16;
        let at = base + usize::try_from(node - self.node_count).ok()? - 16;
        decode(&self.data, at, base).map(|(field, _)| field)
    }
}

/// One field at `at`, and where the next one starts. `base` is what pointers count from.
fn decode(data: &[u8], at: usize, base: usize) -> Option<(Field, usize)> {
    let control = *data.get(at)?;
    let mut next = at + 1;
    let mut kind = control >> 5;
    if kind == 1 {
        let size = (control >> 3) & 3;
        let tail = usize::from(control & 7);
        let count = usize::from(size) + 1;
        let value = data.get(next..next + count)?.iter().fold(0usize, |sum, byte| (sum << 8) | usize::from(*byte));
        let pointer = match size {
            0 => (tail << 8) | value,
            1 => ((tail << 16) | value) + 2048,
            2 => ((tail << 24) | value) + 526_336,
            _ => value,
        };
        let (field, _) = decode(data, base + pointer, base)?;
        return Some((field, next + count));
    }
    if kind == 0 {
        kind = data.get(next)?.wrapping_add(7);
        next += 1;
    }
    let mut size = usize::from(control & 31);
    match size {
        29 => {
            size = 29 + usize::from(*data.get(next)?);
            next += 1;
        }
        30 => {
            size = 285 + data.get(next..next + 2)?.iter().fold(0usize, |sum, byte| (sum << 8) | usize::from(*byte));
            next += 2;
        }
        31 => {
            size = 65_821 + data.get(next..next + 3)?.iter().fold(0usize, |sum, byte| (sum << 8) | usize::from(*byte));
            next += 3;
        }
        _ => {}
    }
    match kind {
        2 => Some((Field::Text(String::from_utf8_lossy(data.get(next..next + size)?).into_owned()), next + size)),
        5 | 6 | 9 | 10 => Some((Field::Number(data.get(next..next + size)?.iter().fold(0u64, |sum, byte| (sum << 8) | u64::from(*byte))), next + size)),
        7 => {
            let mut map = BTreeMap::new();
            for _ in 0..size {
                let (key, after) = decode(data, next, base)?;
                let (value, after) = decode(data, after, base)?;
                next = after;
                if let Field::Text(key) = key {
                    map.insert(key, value);
                }
            }
            Some((Field::Map(map), next))
        }
        11 => {
            let mut list = Vec::with_capacity(size);
            for _ in 0..size {
                let (value, after) = decode(data, next, base)?;
                next = after;
                list.push(value);
            }
            Some((Field::List(list), next))
        }
        14 => Some((Field::Other, next)),
        _ => Some((Field::Other, next + size)),
    }
}
