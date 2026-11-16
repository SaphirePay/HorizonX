//! `ModernUserMap` and `ModernUUIDCache`: `usermap.bin` and `uuids.bin` are Java's own formats,
//! `DataOutputStream.writeUTF` and two longs, so a server keeps them when it moves.
use crate::prelude::*;
use crate::config::flush;
use crate::utils::string::safe_string;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Default)]
struct Cache {
    /// Sanitized name to id, kept in the order `usermap.bin` listed them.
    names: HashMap<String, String>,
    uuids: BTreeSet<String>,
    names_dirty: bool,
    uuids_dirty: bool,
}

pub struct UserMap {
    cache: Mutex<Cache>,
    users: Mutex<HashMap<String, User>>,
}

fn sanitized(name: &str) -> String {
    if settings().is_safe_usermap() { safe_string(name) } else { name.to_owned() }
}

fn name_file() -> PathBuf {
    crate::data_folder().join("usermap.bin")
}

fn uuid_file() -> PathBuf {
    crate::data_folder().join("uuids.bin")
}

fn uuid_text(bytes: &[u8]) -> String {
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}

fn uuid_bytes(uuid: &str) -> Option<[u8; 16]> {
    let hex: String = uuid.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return None;
    }
    let mut out = [0; 16];
    for (at, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&hex[at * 2..at * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// `readUTF`: a two byte length, then modified UTF-8, which is plain UTF-8 for a name.
fn parse_names(bytes: &[u8]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut at = 0;
    while at + 2 <= bytes.len() {
        let length = usize::from(u16::from_be_bytes([bytes[at], bytes[at + 1]]));
        at += 2;
        if at + length + 16 > bytes.len() {
            break;
        }
        let name = String::from_utf8_lossy(&bytes[at..at + length]).into_owned();
        at += length;
        map.insert(name, uuid_text(&bytes[at..at + 16]));
        at += 16;
    }
    map
}

fn write_names(names: &HashMap<String, String>) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, uuid) in names {
        if let Some(id) = uuid_bytes(uuid) {
            out.extend_from_slice(&(name.len() as u16).to_be_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&id);
        }
    }
    out
}

fn write_file(path: &PathBuf, bytes: &[u8]) {
    let temporary = path.with_extension("tmp.bin");
    if let Err(error) = std::fs::write(&temporary, bytes).and_then(|()| std::fs::rename(&temporary, path)) {
        crate::severe(&format!("Error while saving {}: {error}", path.display()));
    }
}

impl UserMap {
    pub fn new() -> Self {
        let names = std::fs::read(name_file()).map(|bytes| parse_names(&bytes)).unwrap_or_default();
        let uuids = std::fs::read(uuid_file()).map(|bytes| bytes.chunks_exact(16).map(uuid_text).collect()).unwrap_or_default();
        Self { cache: Mutex::new(Cache { names, uuids, names_dirty: false, uuids_dirty: false }), users: Mutex::new(HashMap::new()) }
    }
    fn cache(&self) -> std::sync::MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn users(&self) -> std::sync::MutexGuard<'_, HashMap<String, User>> {
        self.users.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// `updateCache`.
    fn update_cache(&self, uuid: &str, name: Option<&str>) {
        let mut cache = self.cache();
        if cache.uuids.insert(uuid.to_owned()) {
            cache.uuids_dirty = true;
        }
        if let Some(name) = name {
            let name = sanitized(name);
            if cache.names.insert(name, uuid.to_owned()).as_deref() != Some(uuid) {
                cache.names_dirty = true;
            }
        }
    }
    /// The five second tick of `ModernUUIDCache`'s writer.
    pub fn save_if_dirty(&self) {
        let (names, uuids) = {
            let mut cache = self.cache();
            let names = std::mem::take(&mut cache.names_dirty).then(|| write_names(&cache.names));
            let uuids = std::mem::take(&mut cache.uuids_dirty).then(|| cache.uuids.iter().filter_map(|uuid| uuid_bytes(uuid)).flatten().collect::<Vec<u8>>());
            (names, uuids)
        };
        if let Some(bytes) = names {
            write_file(&name_file(), &bytes);
        }
        if let Some(bytes) = uuids {
            write_file(&uuid_file(), &bytes);
        }
    }
    /// `blockingSave`, and `shutdown`.
    pub fn blocking_save(&self) {
        {
            let mut cache = self.cache();
            cache.names_dirty = true;
            cache.uuids_dirty = true;
        }
        self.save_if_dirty();
    }
    pub fn get_all_user_uuids(&self) -> Vec<String> {
        self.cache().uuids.iter().cloned().collect()
    }
    pub fn get_user_count(&self) -> usize {
        self.cache().uuids.len()
    }
    pub fn is_cached(&self, uuid: &str) -> bool {
        self.users().contains_key(uuid)
    }
    pub fn get_cached_count(&self) -> usize {
        self.users().len()
    }
    /// `getUser(UUID)`: `None` if they have no file and aren't on.
    pub fn get_user_by_uuid(&self, uuid: &str) -> Option<User> {
        if let Some(user) = self.users().get(uuid) {
            return Some(user.clone());
        }
        let user = self.load_uncached_user(uuid)?;
        self.users().insert(uuid.to_owned(), user.clone());
        Some(user)
    }
    /// `getUser(Player)`.
    pub fn get_user(&self, base: &Player) -> User {
        let uuid = base.get_unique_id();
        // Every move and chat line comes through here: a record already theirs has nothing to update.
        if let Some(user) = self.users().get(&uuid).cloned().filter(|user| user.is_player(base)) {
            return user;
        }
        let user = self.get_user_by_uuid(&uuid).unwrap_or_else(|| User::load(&uuid, Some(base.get_name()), Some(base.clone())));
        if user.player().as_ref() != Some(base) {
            user.update(base.clone());
        }
        self.update_cache(&uuid, Some(&user.get_name()));
        self.users().insert(uuid, user.clone());
        user
    }
    /// `getUser(String)`.
    pub fn get_user_by_name(&self, name: &str) -> Option<User> {
        let uuid = self.cache().names.get(&sanitized(name)).cloned()?;
        let user = self.get_user_by_uuid(&uuid)?;
        if user.is_offline_stub() {
            user.state().offline_name = Some(user.get_last_account_name().unwrap_or_else(|| name.to_owned()));
        }
        Some(user)
    }
    /// `ess.getUser(new OfflinePlayerStub(name, server))`: a record for a name nobody has seen.
    pub fn stub_user(&self, name: &str) -> User {
        let offline = horizon_bukkit::get_offline_player(name);
        let uuid = offline.get_unique_id().and_then(crate::utils::string::to_uuid).unwrap_or_else(|| crate::utils::string::name_uuid_from_bytes(format!("OfflinePlayer:{name}").as_bytes()));
        if let Some(user) = self.get_user_by_uuid(&uuid) {
            return user;
        }
        let user = User::load(&uuid, Some(name), None);
        self.update_cache(&uuid, Some(name));
        self.users().insert(uuid, user.clone());
        user
    }
    /// `addCachedNpcName`.
    pub fn add_cached_npc_name(&self, uuid: &str, name: &str) {
        self.update_cache(uuid, Some(name));
    }
    /// `loadUncachedUser(UUID)`.
    pub fn load_uncached_user(&self, uuid: &str) -> Option<User> {
        if let Some(user) = self.users().get(uuid) {
            return Some(user.clone());
        }
        if let Some(player) = horizon_bukkit::fam_ess::get_player_by_id(uuid) {
            let user = User::load(uuid, Some(player.get_name()), Some(player.clone()));
            self.update_cache(uuid, Some(player.get_name()));
            return Some(user);
        }
        if crate::data_folder().join("userdata").join(format!("{uuid}.yml")).exists() {
            let user = User::load(uuid, None, None);
            let name = user.get_last_account_name();
            user.state().offline_name.clone_from(&name);
            let known = name.as_ref().is_some_and(|name| self.cache().names.contains_key(&sanitized(name)));
            self.update_cache(uuid, name.as_deref().filter(|_| !known));
            return Some(user);
        }
        None
    }
    /// `getNameCache`.
    pub fn get_name_cache(&self) -> HashMap<String, String> {
        self.cache().names.clone()
    }
    /// `getSanitizedName`.
    pub fn get_sanitized_name(&self, name: &str) -> String {
        sanitized(name)
    }
    /// `invalidate`.
    pub fn invalidate(&self, uuid: &str) {
        self.users().remove(uuid);
        let mut cache = self.cache();
        if cache.uuids.remove(uuid) {
            cache.uuids_dirty = true;
        }
        let before = cache.names.len();
        cache.names.retain(|_, known| known != uuid);
        if cache.names.len() != before {
            cache.names_dirty = true;
        }
    }
    /// `removeCache`: Java's other map is only for those online; this one holds until the file is saved.
    pub fn remove_cache(&self, uuid: &str) {
        self.users().remove(uuid);
    }
    /// `shutdown`.
    pub fn shutdown(&self) {
        flush();
        self.blocking_save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_binary_form_is_javas() {
        let mut names = HashMap::new();
        names.insert("bob".to_owned(), "5f4e6a00-1111-2222-3333-444455556666".to_owned());
        let bytes = write_names(&names);
        assert_eq!(&bytes[..5], &[0, 3, b'b', b'o', b'b']);
        assert_eq!(bytes.len(), 21);
        assert_eq!(parse_names(&bytes), names);
    }

    #[test]
    fn a_cut_short_file_stops_cleanly() {
        assert!(parse_names(&[0, 3, b'b']).is_empty());
    }
}
