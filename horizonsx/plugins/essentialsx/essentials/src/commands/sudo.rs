//! `Commandsudo`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), ..Command::new("sudo") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 2, EssError::not_enough());
    let command = get_final_arg(args, 1);
    let multiple = sender.get_user().is_none_or(|user| user.is_authorized("essentials.sudo.multiple"));
    sender.send_tl("sudoRun", crate::args![&args[0], command.clone(), ""]);
    loop_online_players(sender, false, multiple, &args[0], |user| {
        if user.get_name() == sender.get_sender().get_name() {
            return Ok(());
        }
        if user.is_authorized("essentials.sudo.exempt") && sender.is_player() {
            sender.send_tl("sudoExempt", crate::args![user.get_name()]);
            return Ok(());
        }
        let Some(player) = user.player() else { return Ok(()) };
        if command.to_lowercase().starts_with("c:") {
            player.chat(&command[2..]);
            return Ok(());
        }
        if !command.is_empty() {
            let line = format!("/{command}");
            ess().schedule_sync_delayed_task(move || {
                player.chat(&line);
            });
        }
        Ok(())
    })
}
