//! `Commandjump`.
use crate::prelude::*;
use crate::utils::location::get_target;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("jump") };

fn run(user: &User, label: &str, args: &[String]) -> EssResult {
    if args.first().is_some_and(|first| first.contains("lock")) && user.is_authorized("essentials.jump.lock") {
        if user.is_fly_click_jump() {
            user.set_right_click_jump(false);
            user.send_tl("jumpEasterDisable", &[]);
        } else {
            user.set_right_click_jump(true);
            user.send_tl("jumpEasterEnable", &[]);
        }
        return Ok(());
    }
    let (Some(player), Some(at)) = (user.player(), user.get_location()) else {
        return Err(EssError::tl("jumpError", &[]));
    };
    let mut location = get_target(&player)?;
    location.yaw = at.get_yaw();
    location.pitch = at.get_pitch();
    location.y += 1.0;
    let charge = Trade::of_command("jump");
    charge.is_affordable_for(user)?;
    user.get_async_teleport().teleport(&location, Some(&charge), "COMMAND", get_new_exception_future(&user.get_source(), label));
    Err(EssError::NoCharge)
}

fn tab(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 && user.is_authorized("essentials.jump.lock") { vec!["lock".to_owned(), "unlock".to_owned()] } else { Vec::new() })
}
