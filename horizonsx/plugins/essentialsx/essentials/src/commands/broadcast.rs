//! `Commandbroadcast`.
use crate::prelude::*;
use crate::adventure::{escape_tags, legacy_to_mini_with_urls};

pub const COMMAND: Command = Command { run_source: Some(run), ..Command::new("broadcast") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    let message = replace_format(&get_final_arg(args, 0)).replace("\\n", "\n");
    ess().broadcast_tl(None, None, false, "broadcast", crate::args![parsed(legacy_to_mini_with_urls(&escape_tags(&message))), sender.get_display_name()]);
    Ok(())
}
