//! `Commandhelpop`. `HelpopMessageSendEvent` isn't fired: nothing here can listen.
use crate::prelude::*;
use crate::command_source::console_display_name;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_source: Some(tab), ..Command::new("helpop") };

fn run_user(user: &User, _label: &str, args: &[String]) -> EssResult {
    user.set_display_nick();
    send_message(Some(user), &user.get_display_name(), args)
}

fn run_source(_sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    send_message(None, &console_display_name(), args)
}

fn send_message(from: Option<&User>, display: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    let message = strip_format(&get_final_arg(args, 0));
    crate::info(&mini_to_legacy(&tl_literal("helpOp", crate::args![display, message.clone()])));
    let recipients: Vec<User> = ess().get_online_users().into_iter().filter(|user| user.is_authorized("essentials.helpop.receive")).collect();
    if let Some(from) = from
        && !recipients.contains(from)
    {
        from.send_tl("helpOp", crate::args![display, message.clone()]);
    }
    for recipient in &recipients {
        recipient.send_tl("helpOp", crate::args![display, message.clone()]);
    }
    Ok(())
}

fn tab(_sender: &CommandSource, _label: &str, _args: &[String]) -> Option<Vec<String>> {
    Some(Vec::new())
}
