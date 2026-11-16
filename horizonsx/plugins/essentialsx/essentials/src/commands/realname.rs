//! `Commandrealname`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), ..Command::new("realname") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    let first = args.first().ok_or_else(EssError::not_enough)?;
    let lookup = first.to_lowercase();
    let skip = skip_hidden(sender);
    let mut found = false;
    for user in ess().get_online_users() {
        if skip && hidden_from(sender, &user) {
            continue;
        }
        user.set_display_nick();
        if strip_format(&user.get_display_name()).to_lowercase().contains(&lookup) {
            found = true;
            sender.send_tl("realName", crate::args![user.get_display_name(), user.get_name()]);
        }
    }
    if found { Ok(()) } else { Err(EssError::player_not_found()) }
}
