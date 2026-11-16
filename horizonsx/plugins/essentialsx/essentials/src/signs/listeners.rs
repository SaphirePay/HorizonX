//! `SignBlockListener`, `SignEntityListener` and `SignPlayerListener`.
use crate::prelude::*;
use horizon_bukkit::fam_ev_world::BlockPistonEvent;
use super::{BlockSign, all_signs, check_if_block_breaks_signs, enabled_signs, get_username, guarded, is_valid_sign, sign_break, sign_create, sign_interact, success_name, template_name};
use crate::adventure::strip_color;
use crate::utils::material::is_sign;
use horizon_bukkit::blocks::Sign;
use horizon_bukkit::enums::event_priority::{HIGHEST, LOW, LOWEST};
use horizon_bukkit::event::{BlockBreakEvent, PlayerInteractEvent};
use horizon_bukkit::events::{BlockBurnEvent, BlockPistonExtendEvent, BlockPistonRetractEvent, EntityChangeBlockEvent, EntityExplodeEvent};
use horizon_bukkit::fam_ev_world::{BlockIgniteEvent, BlockPlaceEvent, SignChangeEvent};
use horizon_bukkit::{Block, Plugin};

fn top_line(block: &Block) -> String {
    Sign(block.tile()).get_line(0).unwrap_or_default()
}

/// The check every `onSign...` shares: a valid sign here, or one the block holds up.
fn is_sign_or_holds_one(block: &Block) -> bool {
    (is_sign(&block.get_type()) && is_valid_sign(&BlockSign::new(block))) || check_if_block_breaks_signs(block)
}

/// `true` if a sign, or a block a sign guards, says no to harm.
fn refuses_harm(block: &Block) -> bool {
    if is_sign_or_holds_one(block) {
        return true;
    }
    enabled_signs().iter().any(|sign| sign.are_heavy_event_required() && sign.guards(&block.get_type()) && !sign.on_block_harm(block))
}

/// `protectSignsAndBlocks`.
pub fn protect_signs_and_blocks(block: &Block, user: &User) -> EssResult<bool> {
    if check_if_block_breaks_signs(block) {
        if settings().is_debug() {
            crate::info("Prevented that a block was broken next to a sign.");
        }
        return Ok(true);
    }
    if is_sign(&block.get_type()) {
        let line = top_line(block);
        for sign in enabled_signs() {
            if success_name(sign).is_some_and(|name| line.eq_ignore_ascii_case(&name)) && !sign_break(sign, block, user)? {
                return Ok(true);
            }
        }
    }
    for sign in enabled_signs() {
        if sign.are_heavy_event_required() && sign.guards(&block.get_type()) {
            let broke = match sign.on_block_break_by(block, user, &get_username(user)) {
                Err(EssError::MaxMoney) => return Err(EssError::MaxMoney),
                other => guarded(sign, user, other),
            };
            if !broke {
                crate::info("A block was protected by a sign.");
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// `onSignSignChange2`.
fn sign_change_format(event: &mut SignChangeEvent) {
    let Some(user) = event.get_player().and_then(|player| ess().get_user(&player)) else { return };
    for index in 0..4 {
        let line = event.get_line(index).unwrap_or_default().to_owned();
        event.set_line(index, &format_string(&user, "essentials.signs", &line));
    }
    let top = strip_color(event.get_line(0).unwrap_or_default()).to_lowercase().trim().to_owned();
    if !top.is_empty() {
        let enabled = settings().enabled_signs();
        let unprotected = settings().get_unprotected_sign_names();
        for sign in all_signs() {
            let Some(name) = success_name(*sign) else {
                user.send_tl("errorWithMessage", crate::args!["Please report this error to a staff member."]);
                return;
            };
            if top.contains(&strip_color(&name.to_lowercase())) {
                let listed = |list: &[String]| list.iter().any(|known| known.eq_ignore_ascii_case(sign.name()));
                if !listed(&enabled) && listed(&unprotected) {
                    continue;
                }
                event.set_line(0, &top);
            }
        }
    }
}

/// `onSignSignChange`.
fn sign_change_create(event: &mut SignChangeEvent) {
    let Some(user) = event.get_player().and_then(|player| ess().get_user(&player)) else { return };
    let line = event.get_line(0).unwrap_or_default().to_owned();
    for sign in enabled_signs() {
        if success_name(sign).is_some_and(|name| line.eq_ignore_ascii_case(&name)) {
            event.set_cancelled(true);
            return;
        }
        if line.eq_ignore_ascii_case(&template_name(sign)) && !sign_create(sign, event, &user) {
            event.set_cancelled(true);
            return;
        }
    }
}

fn pushed_blocks_refuse(blocks: &[Block]) -> bool {
    blocks.iter().any(refuses_harm)
}

/// `registerListeners`' part for signs: in Java these go in only if any sign is on.
pub fn register(plugin: Plugin) {
    plugin.register_event_with(HIGHEST, true, |event: &mut BlockBreakEvent| {
        if settings().are_signs_disabled() {
            return;
        }
        let Some(user) = ess().get_user(event.get_player()) else { return };
        if protect_signs_and_blocks(event.get_block(), &user).unwrap_or(true) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(LOWEST, true, |event: &mut SignChangeEvent| {
        if !settings().are_signs_disabled() {
            sign_change_format(event);
        }
    });
    plugin.register_event_with(HIGHEST, true, |event: &mut SignChangeEvent| {
        if !settings().are_signs_disabled() {
            sign_change_create(event);
        }
    });
    plugin.register_event_with(LOW, true, |event: &mut BlockPlaceEvent| {
        if settings().are_signs_disabled() {
            return;
        }
        let Some(user) = ess().get_user(event.get_player()) else { return };
        let against = event.get_block_against();
        if is_sign(&against.get_type()) && is_valid_sign(&BlockSign::new(&against)) {
            event.set_cancelled(true);
            return;
        }
        let block = event.get_block().clone();
        if is_sign(&block.get_type()) {
            return;
        }
        for sign in enabled_signs() {
            if sign.are_heavy_event_required() && sign.guards(&block.get_type()) && !guarded(sign, &user, sign.on_block_place(&block, &user, &get_username(&user))) {
                event.set_cancelled(true);
                return;
            }
        }
    });
    plugin.register_event_with(LOW, true, |event: &mut BlockBurnEvent| {
        if !settings().are_signs_disabled() && event.get_block().is_some_and(|block| refuses_harm(&block)) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(LOW, true, |event: &mut BlockIgniteEvent| {
        if !settings().are_signs_disabled() && event.get_block().is_some_and(|block| refuses_harm(&block)) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(LOW, false, |event: &mut BlockPistonExtendEvent| {
        if !settings().are_signs_disabled() && pushed_blocks_refuse(&event.get_blocks()) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(LOW, false, |event: &mut BlockPistonRetractEvent| {
        if settings().are_signs_disabled() {
            return;
        }
        // `isSticky`, then the piston, the block before it, and `getRetractLocation`.
        let Some(base) = event.get_block().filter(|base| base.get_type().name() == "STICKY_PISTON") else { return };
        let (x, y, z) = match event.get_direction().to_uppercase().as_str() {
            "NORTH" => (0, 0, -1),
            "SOUTH" => (0, 0, 1),
            "EAST" => (1, 0, 0),
            "WEST" => (-1, 0, 0),
            "UP" => (0, 1, 0),
            _ => (0, -1, 0),
        };
        if pushed_blocks_refuse(&[base.clone(), base.get_relative(x, y, z), base.get_relative(x * 2, y * 2, z * 2)]) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(LOW, false, |event: &mut EntityExplodeEvent| {
        if settings().are_signs_disabled() {
            return;
        }
        let blocks = event.block_list().clone();
        for block in blocks {
            if is_sign_or_holds_one(&block) {
                event.set_cancelled(true);
                return;
            }
            // The first guard's word goes, either way: this can take an earlier cancel back.
            if let Some(sign) = enabled_signs().into_iter().find(|sign| sign.are_heavy_event_required() && sign.guards(&block.get_type())) {
                event.set_cancelled(!sign.on_block_harm(&block));
                return;
            }
        }
    });
    plugin.register_event_with(LOW, true, |event: &mut EntityChangeBlockEvent| {
        if !settings().are_signs_disabled() && event.get_block().is_some_and(|block| refuses_harm(&block)) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(LOW, false, |event: &mut PlayerInteractEvent| {
        if settings().are_signs_disabled() || (event.get_action() != "RIGHT_CLICK_BLOCK" && event.get_action() != "RIGHT_CLICK_AIR") {
            return;
        }
        let Some(user) = ess().get_user(event.get_player()) else { return };
        let block = if event.is_cancelled() && event.get_action() == "RIGHT_CLICK_AIR" { user.get_target_block(5) } else { event.get_clicked_block().cloned() };
        let Some(block) = block else { return };
        let player = event.get_player().clone();
        if is_sign(&block.get_type()) {
            let line = top_line(&block);
            for sign in enabled_signs() {
                if success_name(sign).is_some_and(|name| line.eq_ignore_ascii_case(&name)) {
                    sign_interact(sign, &block, &player);
                    event.set_cancelled(true);
                    return;
                }
            }
        } else {
            for sign in enabled_signs() {
                if sign.are_heavy_event_required() && sign.guards(&block.get_type()) && !guarded(sign, &user, sign.on_block_interact(&block, &user, &get_username(&user))) {
                    event.set_cancelled(true);
                    return;
                }
            }
        }
    });
}
