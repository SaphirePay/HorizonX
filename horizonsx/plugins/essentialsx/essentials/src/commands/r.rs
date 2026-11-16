//! `Commandr`.
use crate::prelude::*;
use crate::messaging::Recipient;
use crate::utils::format::format_message;

pub const COMMAND: Command = Command { run_source: Some(run), ..Command::new("r") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    let mut message = get_final_arg(args, 0);
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
    let target = from.get_reply_recipient();
    let hidden = |target: &Recipient| match (target, sender.get_player()) {
        (Recipient::User(user), Some(player)) => !settings().is_reply_to_vanished() && user.is_hidden_from(player),
        _ => false,
    };
    let Some(target) = target.filter(|target| !hidden(target)) else {
        from.set_reply_recipient(None);
        return Err(EssError::tl("foreverAlone", &[]));
    };
    from.send_private(&target, &message);
    Ok(())
}
