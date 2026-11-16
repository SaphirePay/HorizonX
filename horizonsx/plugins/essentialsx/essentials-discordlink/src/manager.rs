//! `AccountLinkManager`: the pending codes, and `DiscordLinkService`.
use essentials::prelude::*;
use crate::rolesync::RoleSyncManager;
use crate::storage::AccountStorage;
use essentials_discord::api::InteractionMember;
use rand::Rng;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

const CODE_CHARACTERS: &[u8; 36] = b"abcdefghijklmnopqrstuvwxyz0123456789";

/// `DiscordLinkStatusChangeEvent.Cause`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    SyncPlayer,
    SyncApi,
    UnsyncPlayer,
    UnsyncApi,
    UnsyncLeave,
}

/// `DiscordLinkStatusChangeEvent`.
#[derive(Clone, Debug)]
pub struct DiscordLinkStatusChangeEvent {
    pub user: Option<User>,
    pub member: Option<InteractionMember>,
    pub member_id: String,
    pub linked: bool,
    pub cause: Cause,
}

/// Eight characters nobody is waiting on.
fn generate_code(codes: &HashMap<String, String>) -> String {
    let mut random = rand::rng();
    loop {
        let code: String = (0..8).map(|_| char::from(CODE_CHARACTERS[random.random_range(0..CODE_CHARACTERS.len())])).collect();
        if !codes.contains_key(&code) {
            // If it did clash: buy a lottery ticket.
            return code;
        }
    }
}

/// `createCode`: `Err` holds the code this player already has.
pub fn create_code_in(codes: &mut HashMap<String, String>, uuid: &str) -> Result<String, String> {
    if let Some(previous) = codes.iter().find(|(_, owner)| *owner == uuid).map(|(code, _)| code.clone()) {
        return Err(previous);
    }
    let code = generate_code(codes);
    codes.insert(code.clone(), uuid.to_owned());
    Ok(code)
}

pub struct AccountLinkManager {
    storage: Arc<AccountStorage>,
    role_sync: Arc<RoleSyncManager>,
    code_to_uuid: Mutex<HashMap<String, String>>,
}

impl AccountLinkManager {
    pub fn new(storage: Arc<AccountStorage>, role_sync: Arc<RoleSyncManager>) -> Self {
        Self { storage, role_sync, code_to_uuid: Mutex::new(HashMap::new()) }
    }
    /// `createCode`.
    pub fn create_code(&self, uuid: &str) -> Result<String, String> {
        create_code_in(&mut self.code_to_uuid.lock().unwrap_or_else(|e| e.into_inner()), uuid)
    }
    /// A code for this player, new or the one they have: what the listeners all do with `createCode`.
    pub fn code_for(&self, uuid: &str) -> String {
        self.create_code(uuid).unwrap_or_else(|code| code)
    }
    /// `getPendingUUID`.
    pub fn get_pending_uuid(&self, code: &str) -> Option<String> {
        self.code_to_uuid.lock().unwrap_or_else(|e| e.into_inner()).remove(code)
    }
    /// `getDiscordId`.
    pub fn get_discord_id(&self, uuid: &str) -> Option<String> {
        self.storage.get_discord_id(uuid)
    }
    /// `getUUID`.
    pub fn get_uuid(&self, discord_id: &str) -> Option<String> {
        self.storage.get_uuid(discord_id)
    }
    /// `isLinked(UUID)`.
    pub fn is_uuid_linked(&self, uuid: &str) -> bool {
        self.get_discord_id(uuid).is_some()
    }
    /// `isLinked(String)`.
    pub fn is_discord_linked(&self, discord_id: &str) -> bool {
        self.get_uuid(discord_id).is_some()
    }
    /// `getUser`. Call this on the main thread.
    pub fn get_user(&self, discord_id: &str) -> Option<User> {
        ess().get_user_by_uuid(&self.get_uuid(discord_id)?)
    }
    /// `getAllLinkedPlayers`.
    pub fn get_all_linked_players(&self) -> BTreeMap<String, String> {
        self.storage.get_raw_storage_map()
    }
    /// `unlinkAccount(InteractionMember)`.
    pub fn unlink_member(self: &Arc<Self>, member: &InteractionMember) -> bool {
        self.is_discord_linked(&member.get_id()) && self.remove_member(member, Cause::UnsyncApi)
    }
    /// `unlinkAccount(UUID)`.
    pub fn unlink_uuid(self: &Arc<Self>, uuid: &str) -> bool {
        if !self.is_uuid_linked(uuid) {
            return false;
        }
        if let Some(user) = ess().get_user_by_uuid(uuid) {
            self.remove_user(&user, Cause::UnsyncApi);
        }
        true
    }
    /// `removeAccount(InteractionMember, cause)`.
    pub fn remove_member(self: &Arc<Self>, member: &InteractionMember, cause: Cause) -> bool {
        let id = member.get_id();
        let Some(uuid) = self.get_uuid(&id) else {
            return self.storage.remove_discord_id(&id);
        };
        if !self.storage.remove_discord_id(&id) {
            return false;
        }
        let event = DiscordLinkStatusChangeEvent { user: ess().get_user_by_uuid(&uuid), member: Some(member.clone()), member_id: id.clone(), linked: false, cause };
        crate::listeners::on_user_link_status_change(&event);
        self.role_sync.un_sync(&uuid, &id);
        true
    }
    /// `removeAccount(IUser, cause)`.
    pub fn remove_user(self: &Arc<Self>, user: &User, cause: Cause) -> bool {
        let uuid = user.get_uuid();
        let Some(id) = self.get_discord_id(&uuid).filter(|_| self.storage.remove_uuid(&uuid)) else { return false };
        let (target, member_id) = (user.clone(), id.clone());
        if let Some(api) = essentials_discord::service() {
            api.get_member_by_id(&id).then(move |member| {
                ess().schedule_sync_delayed_task(move || {
                    crate::listeners::on_user_link_status_change(&DiscordLinkStatusChangeEvent { user: Some(target), member, member_id, linked: false, cause });
                });
            });
        }
        self.role_sync.un_sync(&uuid, &id);
        true
    }
    /// `linkAccount`.
    pub fn link_account(self: &Arc<Self>, uuid: &str, member: &InteractionMember) -> bool {
        if self.is_uuid_linked(uuid) || self.is_discord_linked(&member.get_id()) {
            return false;
        }
        self.register_account(uuid, member, Cause::SyncApi);
        true
    }
    /// `registerAccount`.
    pub fn register_account(self: &Arc<Self>, uuid: &str, member: &InteractionMember, cause: Cause) {
        self.storage.add(uuid, &member.get_id());
        self.role_sync.sync(uuid, &member.get_id());
        let event = DiscordLinkStatusChangeEvent { user: ess().get_user_by_uuid(uuid), member: Some(member.clone()), member_id: member.get_id(), linked: true, cause };
        crate::listeners::on_user_link_status_change(&event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_eight_of_the_alphabet() {
        let mut codes = HashMap::new();
        let code = create_code_in(&mut codes, "u").unwrap();
        assert_eq!(code.len(), 8);
        assert!(code.bytes().all(|byte| CODE_CHARACTERS.contains(&byte)));
    }

    #[test]
    fn a_second_code_is_refused_with_the_first() {
        let mut codes = HashMap::new();
        let code = create_code_in(&mut codes, "u").unwrap();
        assert_eq!(create_code_in(&mut codes, "u"), Err(code));
        assert!(create_code_in(&mut codes, "v").is_ok());
    }
}
