//! `RoleSyncManager`. There are no permission backends here but the two Essentials has, and neither can
//! change a player's group: `addToGroup` and `removeFromGroup` have nothing to call, and everyone is in
//! `default`. So roles follow groups, and groups can't follow roles: see GAPS.md.
use essentials::prelude::*;
use crate::manager::Cause;
use crate::settings::DiscordLinkSettings;
use essentials_discord::api::InteractionRole;
use essentials_discord::host::{self, Level};
use horizon_bukkit::enums::event_priority::NORMAL;
use horizon_bukkit::event::PlayerJoinEvent;
use horizon_bukkit::{Plugin, scheduler};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// How long a member lookup may take.
const LOOKUP: Duration = Duration::from_secs(45);
/// One request's longest, and a little.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(20);

/// `Semaphore(5)`.
struct Permits {
    free: Mutex<usize>,
    released: Condvar,
}

impl Permits {
    fn try_acquire(&self, limit: Duration) -> bool {
        let until = Instant::now() + limit;
        let mut free = self.free.lock().unwrap_or_else(|e| e.into_inner());
        while *free == 0 {
            let Some(left) = until.checked_duration_since(Instant::now()) else { return false };
            free = self.released.wait_timeout(free, left).unwrap_or_else(|e| e.into_inner()).0;
        }
        *free -= 1;
        true
    }
    fn release(&self) {
        *self.free.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        self.released.notify_one();
    }
}

/// `isExampleRole`: `0` is how Java reads the file's row of zeros.
pub fn is_example_role(role: &str) -> bool {
    role.trim_start_matches('0').is_empty() || role == "11111111111111111" || role == "22222222222222222" || role == "33333333333333333"
}

/// The groups of a player, on or offline: `getGroups`, of which there's the one.
fn groups_of(user: Option<&User>, primary_only: bool) -> Vec<String> {
    let group = user.map_or_else(|| "default".to_owned(), User::get_group);
    if primary_only { vec![group] } else { user.and_then(User::player).map_or_else(|| vec![group], |player| essentials::perm::get_groups(&player)) }
}

pub struct RoleSyncManager {
    settings: Arc<DiscordLinkSettings>,
    group_to_role: Mutex<HashMap<String, InteractionRole>>,
    role_id_to_group: Mutex<HashMap<String, String>>,
    permits: Permits,
    sync_task: AtomicU32,
    sync_cursor: AtomicUsize,
    /// The async tasks running now, which `shutdown` waits for.
    running: Arc<AtomicUsize>,
    stopped: AtomicBool,
}

/// One running task, for as long as it lasts.
struct Running(Arc<AtomicUsize>);

impl Running {
    fn new(count: &Arc<AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::SeqCst);
        Self(count.clone())
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl RoleSyncManager {
    pub fn new(settings: Arc<DiscordLinkSettings>) -> Arc<Self> {
        Arc::new(Self { settings, group_to_role: Mutex::default(), role_id_to_group: Mutex::default(), permits: Permits { free: Mutex::new(5), released: Condvar::new() }, sync_task: AtomicU32::new(0), sync_cursor: AtomicUsize::new(0), running: Arc::default(), stopped: AtomicBool::new(false) })
    }
    /// The constructor's second half, once the manager exists.
    pub fn start(self: &Arc<Self>, plugin: Plugin) {
        plugin.register_event_with(NORMAL, false, |event: &mut PlayerJoinEvent| {
            let uuid = event.get_player().get_unique_id();
            if let Some(manager) = crate::link_manager()
                && let Some(id) = manager.get_discord_id(&uuid)
                && let Some(role_sync) = crate::role_sync()
            {
                role_sync.sync(&uuid, &id);
            }
        });
        self.on_reload();
        let this = self.clone();
        let period = u64::try_from(self.settings.get_role_sync_resync_delay()).unwrap_or(0).max(1) * 1200;
        let task = scheduler::run_task_timer(1, period, move || this.audit());
        self.sync_task.store(task, Ordering::SeqCst);
    }
    /// The timer: fifty links each time, round and round.
    fn audit(self: &Arc<Self>) {
        if locked(&self.group_to_role).is_empty() && locked(&self.role_id_to_group).is_empty() {
            return;
        }
        let Some(storage) = crate::account_storage() else { return };
        let entries: Vec<(String, String)> = storage.get_raw_storage_map().into_iter().collect();
        if entries.is_empty() {
            self.sync_cursor.store(0, Ordering::SeqCst);
            return;
        }
        let size = entries.len();
        let mut start = self.sync_cursor.load(Ordering::SeqCst);
        if start >= size {
            start = 0;
        }
        let end = (start + 50).min(size);
        self.sync_cursor.store(if end >= size { 0 } else { end }, Ordering::SeqCst);
        for (uuid, discord_id) in &entries[start..end] {
            self.sync(uuid, discord_id);
        }
    }
    /// `shutdown`.
    pub fn shutdown(&self) {
        let task = self.sync_task.swap(0, Ordering::SeqCst);
        if task != 0 {
            scheduler::cancel_task(task);
        }
        // What's running ends with its request: no thread of this module goes on after this.
        self.stopped.store(true, Ordering::SeqCst);
        let until = Instant::now() + SHUTDOWN_WAIT;
        while self.running.load(Ordering::SeqCst) != 0 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    /// `sync`. Call this on the main thread: the groups are the game's. The rest is on its own thread.
    pub fn sync(self: &Arc<Self>, uuid: &str, discord_id: &str) {
        let Some(api) = essentials_discord::service() else { return };
        let user = ess().get_user_by_uuid(uuid);
        let groups = groups_of(user.as_ref(), self.settings.is_role_sync_primary_group_only());
        let group_to_role = locked(&self.group_to_role).clone();
        let remove_roles = self.settings.is_role_sync_remove_roles();
        let unlink_on_leave = self.settings.is_unlink_on_leave();
        let (this, uuid, discord_id) = (self.clone(), uuid.to_owned(), discord_id.to_owned());
        let running = Running::new(&self.running);
        ess().run_task_asynchronously(move || {
            let _running = running;
            if this.stopped.load(Ordering::SeqCst) || !this.permits.try_acquire(Duration::from_secs(5)) {
                return;
            }
            match api.try_member_by_id(&discord_id).wait(LOOKUP) {
                Some(Ok(None)) => {
                    let (target, id) = (uuid.clone(), discord_id.clone());
                    let role_sync = this.clone();
                    ess().schedule_sync_delayed_task(move || {
                        if unlink_on_leave {
                            if let (Some(manager), Some(user)) = (crate::link_manager(), ess().get_user_by_uuid(&target)) {
                                manager.remove_user(&user, Cause::UnsyncLeave);
                            }
                        } else {
                            role_sync.un_sync(&target, &id);
                        }
                    });
                }
                Some(Ok(Some(member))) => {
                    let mut to_add = Vec::new();
                    let mut to_remove = Vec::new();
                    for (group, role) in &group_to_role {
                        if groups.contains(group) && !member.has_role(&role.get_id()) {
                            to_add.push(role.clone());
                        } else if remove_roles && !groups.contains(group) && member.has_role(&role.get_id()) {
                            to_remove.push(role.clone());
                        }
                    }
                    api.modify_member_roles(&member, &to_add, &to_remove).wait(LOOKUP);
                }
                // Java takes a failed lookup for someone who left, and unlinks them: not over a hiccup.
                Some(Err(error)) => host::log(api.host(), Level::Warning, &format!("Failed to fetch Discord member for {uuid} / {discord_id}: {error}")),
                None => host::log(api.host(), Level::Warning, &format!("Failed to fetch Discord member for {uuid} / {discord_id}")),
            }
            this.permits.release();
        });
    }
    /// `unSync`.
    pub fn un_sync(self: &Arc<Self>, uuid: &str, discord_id: &str) {
        if !self.settings.is_role_sync_remove_roles() {
            return;
        }
        let Some(api) = essentials_discord::service() else { return };
        let roles: Vec<InteractionRole> = locked(&self.group_to_role).values().cloned().collect();
        let (this, uuid, discord_id) = (self.clone(), uuid.to_owned(), discord_id.to_owned());
        let running = Running::new(&self.running);
        ess().run_task_asynchronously(move || {
            let _running = running;
            if this.stopped.load(Ordering::SeqCst) || !this.permits.try_acquire(Duration::from_secs(5)) {
                return;
            }
            match api.try_member_by_id(&discord_id).wait(LOOKUP) {
                // Check if the member is no longer in the guild: they don't have any roles anyway.
                Some(Ok(None)) => {}
                Some(Ok(Some(member))) => {
                    api.modify_member_roles(&member, &[], &roles).wait(LOOKUP);
                }
                _ => host::log(api.host(), Level::Warning, &format!("Failed to fetch Discord member for unsync {uuid} / {discord_id}")),
            }
            this.permits.release();
        });
    }
    /// `onReload`.
    pub fn on_reload(&self) {
        let warn = |key: &str, args: &[essentials::i18n::Arg]| essentials::warning(&mini_to_legacy(&tl_literal(key, args)));
        let Some(api) = essentials_discord::service() else { return };
        let groups = vec!["default".to_owned()];
        let mut group_to_role = HashMap::new();
        for (group, role_id) in self.settings.get_role_sync_groups() {
            if is_example_role(&role_id) {
                continue;
            }
            let role = api.get_role(&role_id);
            if !groups.contains(&group) {
                warn("discordLinkInvalidGroup", essentials::args![group, role_id, format!("{groups:?}")]);
                continue;
            }
            let Some(role) = role else {
                warn("discordLinkInvalidRole", essentials::args![role_id, group]);
                continue;
            };
            if role.is_managed() || role.is_public_role() {
                warn("discordLinkInvalidRoleManaged", essentials::args![role.get_name(), role.get_id()]);
                continue;
            }
            if !role.can_interact() {
                warn("discordLinkInvalidRoleInteract", essentials::args![role.get_name(), role.get_id()]);
                continue;
            }
            group_to_role.insert(group, role);
        }
        let mut role_id_to_group = HashMap::new();
        for (role_id, group) in self.settings.get_role_sync_roles() {
            if is_example_role(&role_id) {
                continue;
            }
            let Some(role) = api.get_role(&role_id) else {
                warn("discordLinkInvalidRole", essentials::args![role_id, group]);
                continue;
            };
            if !groups.contains(&group) {
                warn("discordLinkInvalidGroup", essentials::args![group, role_id, format!("{groups:?}")]);
                continue;
            }
            role_id_to_group.insert(role.get_id(), group);
        }
        *locked(&self.group_to_role) = group_to_role;
        *locked(&self.role_id_to_group) = role_id_to_group;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_s_examples_are_skipped() {
        for role in ["0", "000000000000000000", "11111111111111111", "22222222222222222", "33333333333333333"] {
            assert!(is_example_role(role), "{role}");
        }
        assert!(!is_example_role("123456789012345678"));
    }

    #[test]
    fn permits_run_out_and_come_back() {
        let permits = Permits { free: Mutex::new(1), released: Condvar::new() };
        assert!(permits.try_acquire(Duration::ZERO));
        assert!(!permits.try_acquire(Duration::from_millis(5)));
        permits.release();
        assert!(permits.try_acquire(Duration::ZERO));
    }
}
