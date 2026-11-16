//! Java's maps and sets in one type, iterated in the order Java iterates them:
//! - `HashMap`/`HashSet`/`ConcurrentHashMap`: by bucket, as Java's table of that capacity files each key's hash
//!   (`h ^ h >>> 16`, low bits), and within a bucket in the order the keys came. Plugins print help lists, menus and
//!   configs straight out of a `HashMap`, so the same order as Java is the same output as Java.
//! - `LinkedHashMap`/`LinkedHashSet` (and every map jrt makes itself): in the order the keys were first put.
//! - `TreeMap`/`TreeSet`: in the keys' natural order.
//! Lookups go through a hash index of the key string (`V::key`), as before.
use crate::V;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Order {
    #[default]
    Insertion,
    Hash,
    Sorted,
}

#[derive(Clone, Default)]
pub struct JMap {
    /// In the order put; a removed key leaves `None` until the next compaction.
    slots: Vec<Option<Slot>>,
    index: HashMap<String, usize>,
    pub order: Order,
    /// Java's table size for a hashed map: grows as Java's does (doubles past 3/4 full), never shrinks.
    cap: u32,
}

#[derive(Clone)]
struct Slot {
    key: String,
    pair: (V, V),
    hash: i32,
}

/// Java's `HashMap.tableSizeFor`.
pub fn table_size_for(n: i64) -> u32 {
    if n <= 1 {
        return 1;
    }
    (n as u64).next_power_of_two().min(1 << 30) as u32
}

fn string_hash(text: &str) -> i32 {
    text.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(i32::from(c)))
}

/// The `hashCode` Java gives this key, from what `V::key` already worked out (a plugin's own `hashCode` is in the
/// key as `h<hash>`, so it isn't called again here, under the map's lock).
fn java_hash(key: &str, value: &V) -> i32 {
    match value {
        V::Null => 0,
        V::Bool(b) => {
            if *b {
                1231
            } else {
                1237
            }
        }
        V::Char(c) => *c as i32,
        V::Int(i) => *i,
        V::Long(l) => (*l ^ ((*l as u64) >> 32) as i64) as i32,
        V::Double(d) => {
            let bits = if d.is_nan() { 0x7ff8000000000000u64 } else { d.to_bits() };
            (bits ^ (bits >> 32)) as i32
        }
        V::Str(t) => string_hash(t),
        _ => match key.split_at_checked(1) {
            Some(("h", hash)) => hash.parse().unwrap_or(0),
            Some(("o", ptr)) => usize::from_str_radix(ptr.trim_start_matches("0x"), 16).map_or(0, |p| (p >> 4) as i32),
            Some((_, text)) => string_hash(text),
            None => 0,
        },
    }
}

impl JMap {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_order(order: Order) -> Self {
        Self { order, cap: 16, ..Self::default() }
    }
    /// `new HashMap<>(n)`.
    pub fn hashed(initial: u32) -> Self {
        Self { order: Order::Hash, cap: initial.max(1), ..Self::default() }
    }
    pub fn len(&self) -> usize {
        self.index.len()
    }
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }
    pub fn capacity(&self) -> u32 {
        self.cap
    }
    pub fn contains_key(&self, key: &str) -> bool {
        self.index.contains_key(key)
    }
    pub fn get(&self, key: &str) -> Option<&(V, V)> {
        self.index.get(key).and_then(|&i| self.slots[i].as_ref()).map(|s| &s.pair)
    }
    pub fn get_mut(&mut self, key: &str) -> Option<&mut (V, V)> {
        let i = *self.index.get(key)?;
        self.slots[i].as_mut().map(|s| &mut s.pair)
    }
    /// A key already there keeps its place (as `put` does in Java); a new one goes last.
    pub fn insert(&mut self, key: String, pair: (V, V)) -> Option<(V, V)> {
        if let Some(&i) = self.index.get(&key) {
            let slot = self.slots[i].as_mut().expect("an indexed slot");
            return Some(std::mem::replace(&mut slot.pair, pair));
        }
        let hash = java_hash(&key, &pair.0);
        self.index.insert(key.clone(), self.slots.len());
        self.slots.push(Some(Slot { key, pair, hash }));
        if self.cap == 0 {
            self.cap = 16;
        }
        // Java resizes once `++size > threshold`, the threshold being 3/4 of the table.
        while self.order == Order::Hash && self.index.len() as u64 > u64::from(self.cap) * 3 / 4 && self.cap < (1 << 30) {
            self.cap *= 2;
        }
        None
    }
    pub fn remove(&mut self, key: &str) -> Option<(V, V)> {
        let i = self.index.remove(key)?;
        let slot = self.slots[i].take();
        if self.slots.len() > 32 && self.index.len() * 2 < self.slots.len() {
            self.compact();
        }
        slot.map(|s| s.pair)
    }
    fn compact(&mut self) {
        self.slots.retain(Option::is_some);
        self.index.clear();
        for (i, slot) in self.slots.iter().enumerate() {
            if let Some(slot) = slot {
                self.index.insert(slot.key.clone(), i);
            }
        }
    }
    pub fn clear(&mut self) {
        self.slots.clear();
        self.index.clear();
    }
    /// The slots, in Java's iteration order.
    fn walk(&self) -> Vec<usize> {
        let mut live: Vec<usize> = (0..self.slots.len()).filter(|&i| self.slots[i].is_some()).collect();
        match self.order {
            Order::Insertion => {}
            Order::Hash => {
                let mask = self.cap.max(1) - 1;
                let bucket = |i: usize| {
                    let h = self.slots[i].as_ref().map_or(0, |s| s.hash) as u32;
                    (h ^ (h >> 16)) & mask
                };
                live.sort_by_key(|&i| (bucket(i), i));
            }
            Order::Sorted => live.sort_by(|&x, &y| {
                let key = |i: usize| &self.slots[i].as_ref().expect("a live slot").pair.0;
                crate::views::order(key(x), key(y))
            }),
        }
        live
    }
    pub fn iter(&self) -> impl Iterator<Item = (&String, &(V, V))> {
        self.walk().into_iter().map(move |i| {
            let slot = self.slots[i].as_ref().expect("a live slot");
            (&slot.key, &slot.pair)
        })
    }
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.iter().map(|(k, _)| k)
    }
    pub fn values(&self) -> impl Iterator<Item = &(V, V)> {
        self.iter().map(|(_, v)| v)
    }
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut (V, V)> {
        self.iter_mut().map(|(_, v)| v)
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&String, &mut (V, V))> {
        let order = self.walk();
        let mut by_slot: Vec<Option<(&String, &mut (V, V))>> =
            self.slots.iter_mut().map(|s| s.as_mut().map(|s| (&s.key, &mut s.pair))).collect();
        order.into_iter().filter_map(move |i| by_slot[i].take())
    }
    pub fn retain(&mut self, mut keep: impl FnMut(&String, &mut (V, V)) -> bool) {
        for i in 0..self.slots.len() {
            let gone = match self.slots[i].as_mut() {
                Some(slot) => !keep(&slot.key, &mut slot.pair),
                None => false,
            };
            if gone {
                let slot = self.slots[i].take().expect("a live slot");
                self.index.remove(&slot.key);
            }
        }
        if self.slots.len() > 32 && self.index.len() * 2 < self.slots.len() {
            self.compact();
        }
    }
    pub fn append(&mut self, other: &mut JMap) {
        let taken = std::mem::take(other);
        other.order = taken.order;
        other.cap = taken.cap;
        self.extend(taken);
    }
    pub fn entry(&mut self, key: String) -> Entry<'_> {
        Entry { map: self, key }
    }
    /// The same keys in the same order, as a set (`keySet()`): insertion-ordered, so it walks as the map walked.
    pub fn key_set(&self) -> JMap {
        self.iter().map(|(key, (k, _))| (key.clone(), (k.clone(), V::Null))).collect()
    }
    /// A copy of another map, as `new HashMap<>(m)` / `new LinkedHashMap<>(m)` / `new TreeMap<>(m)` makes it.
    pub fn copy_as(&self, order: Order) -> JMap {
        let mut out = match order {
            // `HashMap(Map m)`: a table for `size / 0.75 + 1`.
            Order::Hash => JMap::hashed(table_size_for((self.len() as f64 / 0.75 + 1.0) as i64).max(16)),
            other => JMap::with_order(other),
        };
        out.extend(self.iter().map(|(k, v)| (k.clone(), v.clone())));
        out
    }
}

pub struct Entry<'a> {
    map: &'a mut JMap,
    key: String,
}

impl<'a> Entry<'a> {
    pub fn or_insert_with(self, make: impl FnOnce() -> (V, V)) -> &'a mut (V, V) {
        if !self.map.contains_key(&self.key) {
            self.map.insert(self.key.clone(), make());
        }
        self.map.get_mut(&self.key).expect("just put")
    }
}

impl Extend<(String, (V, V))> for JMap {
    fn extend<T: IntoIterator<Item = (String, (V, V))>>(&mut self, items: T) {
        for (k, v) in items {
            self.insert(k, v);
        }
    }
}

impl FromIterator<(String, (V, V))> for JMap {
    fn from_iter<T: IntoIterator<Item = (String, (V, V))>>(items: T) -> Self {
        let mut map = JMap::new();
        map.extend(items);
        map
    }
}

impl IntoIterator for JMap {
    type Item = (String, (V, V));
    type IntoIter = std::vec::IntoIter<(String, (V, V))>;
    fn into_iter(mut self) -> Self::IntoIter {
        let order = self.walk();
        order.into_iter().filter_map(|i| self.slots[i].take()).map(|s| (s.key, s.pair)).collect::<Vec<_>>().into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::s;

    fn keys(map: &JMap) -> Vec<String> {
        map.values().map(|(k, _)| k.text()).collect()
    }

    #[test]
    fn a_hash_map_walks_as_javas_does() {
        // What Java 21 prints for a `HashMap` of Tebex's command names, put in the order Tebex puts them.
        let mut map = JMap::with_order(Order::Hash);
        for name in ["ban", "checkout", "debug", "forcecheck", "goals", "info", "lookup", "reload", "secret", "sendlink", "help"] {
            map.insert(format!("s{name}"), (s(name), V::Null));
        }
        assert_eq!(keys(&map), ["lookup", "help", "debug", "reload", "forcecheck", "sendlink", "secret", "checkout", "ban", "goals", "info"]);
    }

    #[test]
    fn a_linked_map_keeps_the_order_put_and_a_put_again_keeps_its_place() {
        let mut map = JMap::new();
        for name in ["c", "a", "b"] {
            map.insert(format!("s{name}"), (s(name), V::Null));
        }
        map.insert("sa".into(), (s("a"), V::Int(1)));
        map.remove("sc");
        map.insert("sc".into(), (s("c"), V::Null));
        assert_eq!(keys(&map), ["a", "b", "c"]);
    }

    #[test]
    fn a_tree_map_sorts_numbers_as_numbers() {
        let mut map = JMap::with_order(Order::Sorted);
        for n in [10, 9, 100] {
            map.insert(V::Int(n).key(), (V::Int(n), V::Null));
        }
        assert_eq!(keys(&map), ["9", "10", "100"]);
    }

    #[test]
    fn a_hash_map_grows_its_table_as_java_does() {
        let mut map = JMap::with_order(Order::Hash);
        for n in 0..13 {
            map.insert(V::Int(n * 16).key(), (V::Int(n * 16), V::Null));
        }
        // 13 keys pass 12 (3/4 of 16): 32 buckets, so 16 lands in bucket 16, after 0 but before 32 (bucket 0).
        assert_eq!(map.capacity(), 32);
        assert_eq!(&keys(&map)[..4], ["0", "32", "64", "96"]);
    }
}
