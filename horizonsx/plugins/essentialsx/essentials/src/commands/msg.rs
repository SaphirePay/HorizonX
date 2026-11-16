//! `Commandmsg`.
use crate::prelude::*;
use crate::command_source::console_display_name;
use crate::messaging::Recipient;
use crate::utils::format::format_message;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab_player_first), ..Command::new("msg") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 2, EssError::not_enough());
    let mut message = get_final_arg(args, 1);
    let can_wildcard = sender.is_authorized("essentials.msg.multiple");
    let from = match sender.get_user() {
        Some(user) => {
            fail_if!(user.is_muted(), muted_error(&user));
            message = format_message(&user, "essentials.msg", &message);
            Recipient::User(user)
        }
        None => {
            message = replace_format(&message);
            Recipient::Console
        }
    };
    if args[0].eq_ignore_ascii_case("Console") || args[0].eq_ignore_ascii_case(&console_display_name()) {
        from.send_private(&Recipient::Console, &message);
        return Ok(());
    }
    loop_online_players(sender, false, can_wildcard, &args[0], |receiver| {
        from.send_private(&Recipient::User(receiver.clone()), &message);
        Ok(())
    })
}
