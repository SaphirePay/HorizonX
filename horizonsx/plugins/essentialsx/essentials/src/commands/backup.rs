//! `Commandbackup`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), ..Command::new("backup") };

fn run(sender: &CommandSource, _label: &str, _args: &[String]) -> EssResult {
    let command = settings().get_backup_command().unwrap_or_default();
    fail_if!(command.is_empty() || command.eq_ignore_ascii_case("save-all"), EssError::tl("backupDisabled", &[]));
    ess().backup().run();
    sender.send_tl("backupStarted", &[]);
    Ok(())
}
