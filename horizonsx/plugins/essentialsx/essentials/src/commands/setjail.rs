//! `Commandsetjail`.
use crate::prelude::*;
use crate::utils::string::sanitize_string;

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("setjail") };

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let name = args.first().ok_or_else(EssError::not_enough)?;
    let Some(location) = user.get_location() else { return Ok(()) };
    ess().jails().set_jail(name, &location);
    user.send_tl("jailSet", crate::args![sanitize_string(name)]);
    Ok(())
}
