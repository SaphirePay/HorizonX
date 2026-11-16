//! `DiscordLinkSettings`.
use essentials::config::{Config, scalar_string};
use serde_yaml::Value;
use std::path::PathBuf;
use std::sync::{RwLock, RwLockReadGuard};

/// `LinkPolicy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkPolicy {
    Kick,
    Freeze,
    None,
}

impl LinkPolicy {
    /// `fromName`.
    fn from_name(name: &str) -> Self {
        match name.to_lowercase().as_str() {
            "kick" => Self::Kick,
            "freeze" => Self::Freeze,
            _ => Self::None,
        }
    }
}

pub struct DiscordLinkSettings {
    config: RwLock<Config>,
    link_policy: RwLock<LinkPolicy>,
}

/// `getStringMap`: keys and values both as text, numbers too.
fn string_map(rows: Vec<(String, Value)>) -> Vec<(String, String)> {
    rows.into_iter().filter_map(|(key, value)| Some((key, scalar_string(&value)?))).collect()
}

impl DiscordLinkSettings {
    pub fn new(file: PathBuf, template: &'static str) -> Self {
        let settings = Self { config: RwLock::new(Config::with_template(file, template)), link_policy: RwLock::new(LinkPolicy::None) };
        settings.reload_config();
        settings
    }
    fn config(&self) -> RwLockReadGuard<'_, Config> {
        self.config.read().unwrap_or_else(|e| e.into_inner())
    }
    pub fn get_link_policy(&self) -> LinkPolicy {
        *self.link_policy.read().unwrap_or_else(|e| e.into_inner())
    }
    pub fn is_block_unlinked_chat(&self) -> bool {
        self.config().get_boolean("block-unlinked-chat", false)
    }
    pub fn is_unlink_on_leave(&self) -> bool {
        self.config().get_boolean("unlink-on-leave", true)
    }
    pub fn is_relay_mail(&self) -> bool {
        self.config().get_boolean("relay-mail", true)
    }
    pub fn is_role_sync_remove_roles(&self) -> bool {
        self.config().get_boolean("role-sync.remove-roles", true)
    }
    pub fn is_role_sync_remove_groups(&self) -> bool {
        self.config().get_boolean("role-sync.remove-groups", true)
    }
    pub fn get_role_sync_resync_delay(&self) -> i32 {
        self.config().get_int("role-sync.resync-delay", 5)
    }
    pub fn is_role_sync_primary_group_only(&self) -> bool {
        self.config().get_boolean("role-sync.primary-group-only", false)
    }
    pub fn get_role_sync_groups(&self) -> Vec<(String, String)> {
        string_map(self.config().section_map("role-sync.groups"))
    }
    pub fn get_role_sync_roles(&self) -> Vec<(String, String)> {
        string_map(self.config().section_map("role-sync.roles"))
    }
    /// `reloadConfig`. `onReload` is the caller's.
    pub fn reload_config(&self) {
        self.config.write().unwrap_or_else(|e| e.into_inner()).load();
        let policy = LinkPolicy::from_name(&self.config().string_or("link-policy", "none"));
        *self.link_policy.write().unwrap_or_else(|e| e.into_inner()) = policy;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_policy_it_does_not_know_is_none() {
        assert_eq!(LinkPolicy::from_name("KICK"), LinkPolicy::Kick);
        assert_eq!(LinkPolicy::from_name("freeze"), LinkPolicy::Freeze);
        assert_eq!(LinkPolicy::from_name("whatever"), LinkPolicy::None);
    }

    #[test]
    fn the_shipped_file_reads_as_java_reads_it() {
        let folder = std::env::temp_dir().join(format!("essx-link-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let settings = DiscordLinkSettings::new(folder.join("config.yml"), include_str!("../config.yml"));
        assert_eq!(settings.get_link_policy(), LinkPolicy::None);
        assert!(settings.is_unlink_on_leave());
        assert_eq!(settings.get_role_sync_resync_delay(), 5);
        let groups = settings.get_role_sync_groups();
        assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|(_, role)| crate::rolesync::is_example_role(role)));
        let roles = settings.get_role_sync_roles();
        assert_eq!(roles.len(), 2);
        assert!(roles.iter().all(|(role, _)| crate::rolesync::is_example_role(role)));
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
