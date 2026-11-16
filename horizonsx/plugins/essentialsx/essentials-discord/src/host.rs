//! What the Discord side asks of the game. Everything else in this crate that runs on a thread of its
//! own goes through here, so that no game call is made off the main thread, and so that the client,
//! the relay and the webhooks can be tried against a fake server with no game behind them.
use crate::client::secret;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Severe,
}

/// A message from Discord, ready to be shown: the end of `DiscordListener.onMessageReceived`.
#[derive(Clone, Debug)]
pub struct Relay {
    /// The channel names this came through: `getKeysFromChannelId`.
    pub keys: Vec<String>,
    pub formatted: String,
    /// `getContentRaw`.
    pub raw: String,
    pub member: Option<crate::api::InteractionMember>,
    pub channel: crate::api::InteractionChannel,
}

pub type Task = Box<dyn FnOnce() + Send>;
/// `CmdCallback`: a command's output, once it's in.
pub type Feedback = Box<dyn Fn(String) + Send + Sync>;

pub trait Host: Send + Sync + 'static {
    /// `Bukkit.getScheduler().runTask`.
    fn run_main(&self, task: Task);
    /// `Bukkit.getServer().isPrimaryThread()`.
    fn is_main(&self) -> bool;
    /// The plugin's logger. Use `log` below, which takes secrets out first.
    fn write_log(&self, level: Level, text: &str);
    /// `isDebug`.
    fn is_debug(&self) -> bool;
    /// On the main thread: who should see this, and showing it.
    fn deliver(&self, relay: Relay);
    /// On the main thread: `Bukkit.dispatchCommand` with the output to `feedback`.
    fn dispatch_command(&self, line: &str, feedback: Feedback);
}

/// The logger. Nothing reaches a log but through this.
pub fn log(host: &Arc<dyn Host>, level: Level, text: &str) {
    host.write_log(level, &secret::scrub(text));
}

/// The game. Its calls are made on the main thread alone: `run_main` from any other.
pub struct GameHost;

impl Host for GameHost {
    fn run_main(&self, task: Task) {
        let mut task = Some(task);
        horizon_bukkit::scheduler::run_task(move || {
            if let Some(task) = task.take() {
                task();
            }
        });
    }
    fn is_main(&self) -> bool {
        horizon_bukkit::is_primary_thread()
    }
    fn write_log(&self, level: Level, text: &str) {
        let write = {
            let text = text.to_owned();
            move || match level {
                Level::Info => essentials::info(&text),
                Level::Warning => essentials::warning(&text),
                Level::Severe => essentials::severe(&text),
            }
        };
        if self.is_main() {
            write();
        } else {
            self.run_main(Box::new(write));
        }
    }
    fn is_debug(&self) -> bool {
        essentials::ess::settings().is_debug()
    }
    fn deliver(&self, relay: Relay) {
        crate::listeners::deliver(relay);
    }
    fn dispatch_command(&self, line: &str, feedback: Feedback) {
        crate::listeners::dispatch_command(line, feedback);
    }
}
