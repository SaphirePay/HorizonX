//! `Commandeditsign`.
use crate::prelude::*;
use crate::adventure::strip_color;
use crate::utils::format::unformat_string_for;
use crate::utils::number::is_positive_int;
use horizon_bukkit::Block;
use horizon_bukkit::blocks::Sign;
use horizon_bukkit::fam_core::get_plugin_manager;
use horizon_bukkit::fam_ev_world::SignChangeEvent;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("editsign") };

/// `wrapSign`: the sign, and whether the player is on its front.
fn wrap_sign(block: &Block, user: &User) -> Option<(Sign, bool)> {
    let player = user.player()?;
    let name = block.get_type().name();
    let eyes = player.get_eye_location();
    let (to_x, to_z) = (eyes.x - (f64::from(block.get_x()) + 0.5), eyes.z - (f64::from(block.get_z()) + 0.5));
    let (face_x, face_z) = if name.ends_with("WALL_SIGN") || name.ends_with("WALL_HANGING_SIGN") {
        match block.get_property("facing").as_deref() {
            Some("north") => (0.0, -1.0),
            Some("south") => (0.0, 1.0),
            Some("west") => (-1.0, 0.0),
            _ => (1.0, 0.0),
        }
    } else {
        let angle = block.get_property("rotation")?.parse::<f64>().ok()? * 22.5_f64.to_radians();
        (-angle.sin(), angle.cos())
    };
    Some((Sign(block.tile()), to_x * face_x + to_z * face_z > 0.0))
}

fn is_sign(block: &Block) -> bool {
    block.get_type().name().ends_with("_SIGN")
}

/// `callSignEvent`: `true` if the sign stays as it is.
fn store(mut sign: Sign, block: &Block, front: bool, user: &User, lines: &[String]) -> bool {
    if sign.is_waxed() && !user.is_authorized("essentials.editsign.waxed.exempt") {
        return true;
    }
    let Some(mut event) = user.player().and_then(|player| SignChangeEvent::new(block, &player, lines, front)) else { return true };
    get_plugin_manager().call_event(&mut event);
    if event.is_cancelled() {
        if settings().is_debug() {
            crate::info(&format!("SignChangeEvent canceled for /editsign execution by {}", user.get_name()));
        }
        return true;
    }
    sign.set_side_lines(front, event.get_lines());
    sign.update();
    false
}

fn no_line() -> EssError {
    EssError::tl("editsignCommandNoLine", &[])
}

fn line_of(text: &str) -> EssResult<usize> {
    let number: i32 = text.parse().map_err(|_| no_line())?;
    usize::try_from(number - 1).ok().filter(|line| *line < 4).ok_or_else(no_line)
}

fn run(user: &User, label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty() || (args.len() > 1 && !is_int(&args[1])), EssError::not_enough());
    let Some((block, (sign, front))) = user.get_target_block(5).filter(is_sign).and_then(|block| {
        let wrapped = wrap_sign(&block, user)?;
        Some((block, wrapped))
    }) else {
        return Err(EssError::tl("editsignCommandTarget", &[]));
    };
    let mut lines = sign.get_side_lines(front);
    let word = args[0].to_lowercase();
    match word.as_str() {
        "set" if args.len() > 2 => {
            let line = line_of(&args[1])?;
            let text = format_string(user, "essentials.editsign", &get_final_arg(args, 2)).trim().to_owned();
            fail_if!(strip_color(&text).encode_utf16().count() > 15 && !user.is_authorized("essentials.editsign.unlimited"), EssError::tl("editsignCommandLimit", &[]));
            lines[line].clone_from(&text);
            if !store(sign, &block, front, user, &lines) {
                user.send_tl("editsignCommandSetSuccess", crate::args![line + 1, text]);
            }
        }
        "clear" if args.len() == 1 => {
            lines.fill(String::new());
            if !store(sign, &block, front, user, &lines) {
                user.send_tl("editsignCommandClear", &[]);
            }
        }
        "clear" => {
            let line = line_of(&args[1])?;
            lines[line].clear();
            if !store(sign, &block, front, user, &lines) {
                user.send_tl("editsignCommandClearLine", crate::args![line + 1]);
            }
        }
        "copy" if args.len() == 1 => {
            let copy = lines.iter().map(|line| unformat_string_for(user, "essentials.editsign", line)).collect::<Vec<_>>();
            for (index, line) in copy.into_iter().enumerate() {
                user.set_sign_copy_line(index, line);
            }
            user.send_tl("editsignCopy", crate::args![label]);
        }
        "copy" => {
            let line = line_of(&args[1])?;
            user.set_sign_copy_line(line, unformat_string_for(user, "essentials.editsign", &lines[line]));
            user.send_tl("editsignCopyLine", crate::args![line + 1, label]);
        }
        "paste" if args.len() == 1 => {
            for (index, copied) in user.get_sign_copy().iter().enumerate() {
                lines[index] = format_string(user, "essentials.editsign", copied);
            }
            if !store(sign, &block, front, user, &lines) {
                user.send_tl("editsignPaste", crate::args![label]);
            }
        }
        "paste" => {
            let line = line_of(&args[1])?;
            lines[line] = format_string(user, "essentials.editsign", &user.get_sign_copy()[line]);
            if !store(sign, &block, front, user, &lines) {
                user.send_tl("editsignPasteLine", crate::args![line + 1, label]);
            }
        }
        _ => return Err(EssError::not_enough()),
    }
    Ok(())
}

fn tab(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => ["set", "clear", "copy", "paste"].map(str::to_owned).to_vec(),
        2 => ["1", "2", "3", "4"].map(str::to_owned).to_vec(),
        3 if args[0].eq_ignore_ascii_case("set") && is_positive_int(&args[1]) => {
            let line: usize = args[1].parse().unwrap_or(0);
            let found = user.get_target_block(5).filter(is_sign).and_then(|block| wrap_sign(&block, user));
            found.filter(|_| line <= 4).and_then(|(sign, front)| sign.get_side_lines(front).get(line.wrapping_sub(1)).cloned()).map(|text| unformat_string_for(user, "essentials.editsign", &text)).into_iter().collect()
        }
        _ => Vec::new(),
    })
}
