//! `Commandping`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), ..Command::new("ping") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    if args.is_empty() {
        sender.send_tl("pong", &[]);
    } else {
        sender.send_message(&replace_format(&get_final_arg(args, 0)));
    }
    Ok(())
}
