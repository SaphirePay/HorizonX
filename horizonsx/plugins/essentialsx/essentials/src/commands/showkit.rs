//! `Commandshowkit`.
use crate::prelude::*;
use crate::kits::Kit;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("showkit") };

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let [names] = args else {
        return Err(EssError::not_enough());
    };
    for name in names.to_lowercase().split(',') {
        user.send_tl("kitContains", crate::args![name]);
        for item in Kit::new(name)?.get_items()? {
            user.send_tl("kitItem", crate::args![item]);
        }
    }
    Ok(())
}

fn tab(_user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 { ess().kits().get_kit_keys() } else { Vec::new() })
}
