//! `BalanceTopImpl`.
use crate::prelude::*;
use std::sync::Mutex;

#[derive(Clone, Debug)]
pub struct Entry {
    pub uuid: String,
    pub display_name: String,
    pub balance: BigDecimal,
}

#[derive(Default)]
struct State {
    entries: Vec<Entry>,
    total: Option<BigDecimal>,
    age: i64,
    locked: bool,
}

#[derive(Default)]
pub struct BalanceTop {
    state: Mutex<State>,
}

impl BalanceTop {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// `calculateBalanceTopMap`.
    pub fn calculate(&self) {
        let mut entries = Vec::new();
        let mut total = BigDecimal::zero();
        for uuid in ess().users().get_all_user_uuids() {
            let Some(user) = ess().users().load_uncached_user(&uuid) else { continue };
            if !settings().is_npcs_in_balance_ranking() && user.is_npc() {
                continue;
            }
            if user.is_baltop_exempt() {
                continue;
            }
            let money = user.get_money();
            total = total.add(&money);
            entries.push(Entry { uuid, display_name: entry_name(&user), balance: money });
        }
        entries.sort_by(|one, other| other.balance.cmp(&one.balance));
        if let Ok(limit) = usize::try_from(settings().get_baltop_entry_limit()) {
            entries.truncate(limit);
        }
        let mut state = self.state();
        state.entries = entries;
        state.total = Some(total);
        state.age = now_ms();
        state.locked = false;
    }
    /// `calculateBalanceTopMapAsync`: `then` runs once the table is ready, on the server thread.
    pub fn calculate_async(&self, then: impl FnOnce() + Send + 'static) {
        self.state().locked = true;
        ess().run_task_asynchronously(move || {
            ess().balance_top().calculate();
            let mut then = Some(then);
            ess().schedule_sync_delayed_task(move || {
                if let Some(then) = then.take() {
                    then();
                }
            });
        });
    }
    /// `getBalanceTopCache`.
    pub fn get_balance_top_cache(&self) -> Vec<Entry> {
        self.state().entries.clone()
    }
    /// `getCacheAge`.
    pub fn get_cache_age(&self) -> i64 {
        self.state().age
    }
    /// `getBalanceTopTotal`.
    pub fn get_balance_top_total(&self) -> BigDecimal {
        self.state().total.clone().unwrap_or_else(BigDecimal::zero)
    }
    /// `isCacheLocked`.
    pub fn is_cache_locked(&self) -> bool {
        self.state().locked
    }
}

fn entry_name(user: &User) -> String {
    if user.is_offline_stub() {
        user.get_last_account_name().unwrap_or_default()
    } else if user.is_hidden() {
        user.get_name()
    } else {
        user.get_display_name()
    }
}
