//! A fast, non-cryptographic hasher (the Fx hash used by rustc) for hot maps
//! keyed by small integers, such as chunk coordinates. Iteration order of
//! such maps carries no meaning: the default hasher's is random anyway.
use std::hash::{BuildHasherDefault, Hasher};

const SEED: u64 = 0xf135_7aea_2e62_a9c5;

#[derive(Clone, Copy, Default)]
pub struct FxHasher {
    hash: u64,
}
impl FxHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.hash = self.hash.wrapping_add(word).wrapping_mul(SEED);
    }
}
impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let (words, rest) = bytes.as_chunks::<8>();
        for word in words {
            self.add(u64::from_le_bytes(*word));
        }
        if !rest.is_empty() {
            let mut word = [0u8; 8];
            word[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(word));
        }
    }
    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(u64::from(i));
    }
    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add(u64::from(i));
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(u64::from(i));
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
    #[inline]
    fn write_i32(&mut self, i: i32) {
        self.add(u64::from(i as u32));
    }
    #[inline]
    fn finish(&self) -> u64 {
        // The multiply leaves the low bits weakest; hash tables use them.
        self.hash.rotate_left(26)
    }
}
pub type FxBuild = BuildHasherDefault<FxHasher>;
pub type FxHashMap<K, V> = std::collections::HashMap<K, V, FxBuild>;
pub type FxHashSet<K> = std::collections::HashSet<K, FxBuild>;

