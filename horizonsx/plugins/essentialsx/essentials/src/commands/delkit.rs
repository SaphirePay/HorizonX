//! `Commanddelkit`.
use crate::prelude::*;
use crate::kits::Kit;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("delkit") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    let Some(first) = args.first() else {
        let list = ess().kits().list_kits(None)?;
        sender.send_tl(if list.is_empty() { "noKits" } else { "kits" }, crate::args![list]);
        return Err(EssError::NoCharge);
    };
    let name = ess().kits().match_kit(first).ok_or_else(|| EssError::tl("kitNotFound", &[]))?;
    let kit = Kit::new(&name)?;
    if let Some(user) = sender.get_user() {
        kit.check_perms(&user)?;
    }
    ess().kits().remove_kit(&name);
    sender.send_tl("deleteKit", crate::args![name]);
    Ok(())
}

fn tab(_sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 { ess().kits().get_kit_keys() } else { Vec::new() })
}
