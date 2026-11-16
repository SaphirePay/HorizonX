//! `Backup`: `backup.command` on a timer, with saving off around it.
use crate::prelude::*;
use horizon_bukkit::{dispatch_command, scheduler};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
struct State {
    running: bool,
    task: Option<u32>,
    active: bool,
}

#[derive(Default)]
pub struct Backup {
    state: Mutex<State>,
    pending_shutdown: AtomicBool,
}

impl Backup {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// `onPlayerJoin`.
    pub fn on_player_join(&self) {
        self.start_task();
    }
    /// `stopTask`.
    pub fn stop_task(&self) {
        let mut state = self.state();
        state.running = false;
        if let Some(task) = state.task.take() {
            scheduler::cancel_task(task);
        }
    }
    /// `startTask`.
    pub fn start_task(&self) {
        let mut state = self.state();
        if state.running {
            return;
        }
        let interval = settings().get_backup_interval() * 1200;
        if interval < 1200 {
            return;
        }
        state.task = Some(scheduler::run_task_timer(interval as u64, interval as u64, || ess().backup().run()));
        state.running = true;
    }
    /// `setPendingShutdown`.
    pub fn set_pending_shutdown(&self, shutdown: bool) {
        self.pending_shutdown.store(shutdown, Ordering::SeqCst);
    }
    /// Is a backup running: `getTaskLock` not done.
    pub fn is_active(&self) -> bool {
        self.state().active
    }
    /// `run`.
    pub fn run(&self) {
        if self.state().active {
            return;
        }
        let Some(command) = settings().get_backup_command().filter(|command| !command.is_empty()) else { return };
        self.state().active = true;
        if command.eq_ignore_ascii_case("save-all") {
            dispatch_command("save-all");
            self.state().active = false;
            return;
        }
        crate::info(&mini_to_legacy(&tl_literal("backupStarted", &[])));
        dispatch_command("save-all");
        dispatch_command("save-off");
        ess().run_task_asynchronously(move || {
            let mut words = command.split(' ');
            let program = words.next().unwrap_or_default();
            let child = Command::new(program).args(words).current_dir(horizon_bukkit::server::server_folder()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
            match child {
                Ok(mut child) => {
                    if let Some(output) = child.stdout.take() {
                        for line in BufReader::new(output).lines().map_while(Result::ok) {
                            crate::info(&line);
                        }
                    }
                    let _ = child.wait();
                }
                Err(error) => crate::severe(&format!("An error occurred while building the backup child process: {error}")),
            }
            if !ess().backup().pending_shutdown.load(Ordering::SeqCst) {
                ess().schedule_sync_delayed_task(|| {
                    dispatch_command("save-on");
                    if !settings().is_always_run_backup() && ess().get_online_users().is_empty() {
                        ess().backup().stop_task();
                    }
                    ess().backup().state().active = false;
                    crate::info(&mini_to_legacy(&tl_literal("backupFinished", &[])));
                });
            } else {
                ess().backup().state().active = false;
            }
        });
    }
}
