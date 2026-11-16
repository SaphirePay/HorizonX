//! `Commandmotd`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), ..Command::new("motd") };

fn run(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    show_text_file(sender, "motd", label, args)
}
