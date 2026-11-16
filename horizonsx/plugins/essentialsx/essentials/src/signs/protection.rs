//! `SignProtection`, which Java keeps though it's marked for removal.
use crate::prelude::*;
use super::{BlockSign, EssentialsSign, ISign, reads_as_owner, success_name};
use crate::trade::OverflowType;
use crate::utils::material::is_sign;
use horizon_bukkit::blocks::Sign;
use horizon_bukkit::Block;
use std::collections::BTreeMap;

pub struct SignProtection;

#[derive(Clone, Copy, PartialEq)]
enum State {
    NotAllowed,
    Allowed,
    NoSign,
    Owner,
}

type Connected = BTreeMap<(i32, i32, i32), (Block, State)>;

fn protects(material: &Material) -> bool {
    matches!(material.name().as_str(), "CHEST" | "FURNACE" | "DISPENSER")
}

fn adjacent(block: &Block) -> [Block; 6] {
    [block.get_relative(0, 0, -1), block.get_relative(0, 0, 1), block.get_relative(1, 0, 0), block.get_relative(-1, 0, 0), block.get_relative(0, -1, 0), block.get_relative(0, 1, 0)]
}

fn has_adjacent_block(block: &Block) -> bool {
    adjacent(block).iter().any(|near| protects(&near.get_type()))
}

fn check_sign(sign: &dyn ISign, who: Option<(&User, &str)>) -> State {
    let Some((user, username)) = who else { return State::NotAllowed };
    if user.is_authorized("essentials.signs.protection.override") || reads_as_owner(user, sign, 3) {
        return State::Owner;
    }
    for index in 1..=2 {
        let line = sign.get_line(index);
        if line.len() >= 2 && line.starts_with('(') && line.ends_with(')') && user.in_group(&line[1..line.len() - 1]) {
            return State::Allowed;
        }
        if line.eq_ignore_ascii_case(username) {
            return State::Allowed;
        }
    }
    State::NotAllowed
}

fn check_block(block: &Block, who: Option<(&User, &str)>) -> State {
    if is_sign(&block.get_type()) {
        let sign = BlockSign::new(block);
        if Some(sign.get_line(0)) == success_name(&SignProtection) {
            return check_sign(&sign, who);
        }
    }
    State::NoSign
}

fn connected(block: &Block, signs: &mut Connected, who: Option<(&User, &str)>, depth: i32) {
    for near in adjacent(block) {
        let key = (near.get_x(), near.get_y(), near.get_z());
        if signs.contains_key(&key) {
            continue;
        }
        signs.insert(key, (near.clone(), check_block(&near, who)));
        if protects(&near.get_type()) && depth > 0 {
            connected(&near, signs, who, depth - 1);
        }
    }
}

fn connected_signs(block: &Block, who: Option<(&User, &str)>, secure: bool) -> Connected {
    let mut signs = Connected::new();
    connected(block, &mut signs, who, if secure { 4 } else { 2 });
    signs
}

fn is_block_protected(block: &Block, who: Option<(&User, &str)>, secure: bool) -> State {
    let signs = connected_signs(block, who, secure);
    let mut state = State::NoSign;
    for (_, found) in signs.values() {
        if *found == State::Allowed {
            state = *found;
        } else if *found == State::NotAllowed && state != State::Allowed {
            state = *found;
        }
    }
    if (!secure || state == State::NoSign) && signs.values().any(|(_, found)| *found == State::Owner) {
        return State::Owner;
    }
    state
}

fn check_if_signs_are_broken(block: &Block, user: &User, username: &str) -> EssResult {
    for (sign, state) in connected_signs(block, Some((user, username)), false).values() {
        if *state != State::NoSign && !has_adjacent_block(sign) {
            block.set_type(&Material::of("AIR"));
            Trade::of_item(ItemStack::new(sign.get_type(), 1)).pay_with(user, OverflowType::Drop)?;
        }
    }
    Ok(())
}

fn name_of(block: &Block) -> String {
    block.get_type().name().to_lowercase()
}

impl EssentialsSign for SignProtection {
    fn name(&self) -> &'static str {
        "Protection"
    }
    fn on_create(&self, sign: &mut dyn ISign, user: &User, username: &str) -> EssResult<bool> {
        sign.set_line(3, &format!("\u{a7}4{username}"));
        if has_adjacent_block(sign.get_block()) {
            let state = is_block_protected(sign.get_block(), Some((user, username)), true);
            if state == State::NoSign || state == State::Owner || user.is_authorized("essentials.signs.protection.override") {
                sign.set_line(3, &format!("\u{a7}1{username}"));
                return Ok(true);
            }
        }
        user.send_tl("signProtectInvalidLocation", &[]);
        Ok(false)
    }
    fn on_break(&self, sign: &mut dyn ISign, user: &User, username: &str) -> EssResult<bool> {
        Ok(check_sign(sign, Some((user, username))) == State::Owner)
    }
    fn are_heavy_event_required(&self) -> bool {
        true
    }
    fn guards(&self, material: &Material) -> bool {
        protects(material)
    }
    fn on_block_place(&self, block: &Block, user: &User, username: &str) -> EssResult<bool> {
        for near in adjacent(block) {
            let state = is_block_protected(&near, Some((user, username)), true);
            if (state == State::Allowed || state == State::NotAllowed) && !user.is_authorized("essentials.signs.protection.override") {
                user.send_tl("noPlacePermission", crate::args![name_of(block)]);
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn on_block_interact(&self, block: &Block, user: &User, username: &str) -> EssResult<bool> {
        let state = is_block_protected(block, Some((user, username)), false);
        if state != State::NotAllowed || user.is_authorized("essentials.signs.protection.override") {
            return Ok(true);
        }
        user.send_tl("noAccessPermission", crate::args![name_of(block)]);
        Ok(false)
    }
    fn on_block_break_by(&self, block: &Block, user: &User, username: &str) -> EssResult<bool> {
        let state = is_block_protected(block, Some((user, username)), false);
        if state == State::Owner || state == State::NoSign || user.is_authorized("essentials.signs.protection.override") {
            check_if_signs_are_broken(block, user, username)?;
            return Ok(true);
        }
        user.send_tl("noDestroyPermission", crate::args![name_of(block)]);
        Ok(false)
    }
    fn on_block_harm(&self, block: &Block) -> bool {
        is_block_protected(block, None, false) == State::NoSign
    }
}

/// `isBlockProtected(block)`: is there a protection sign on it, or on what's beside it. For EssentialsAntiBuild's kin.
pub fn is_sign_protected(block: &Block) -> bool {
    let marked = |near: &Block| is_sign(&near.get_type()) && Sign(near.tile()).get_line(0).is_some_and(|line| line.eq_ignore_ascii_case("\u{a7}1[Protection]"));
    adjacent(block).iter().any(|near| marked(near) || (protects(&near.get_type()) && adjacent(near).iter().any(marked)))
}
