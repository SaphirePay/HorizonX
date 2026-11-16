//! `Commandskull`. Java's profile lookup runs off the server thread; here a name is written on the
//! stack and the game resolves it, as it does for a head with a name and no id.
use crate::prelude::*;
use crate::utils::base64::{decode, encode};
use horizon_bukkit::meta::SkullMeta;
use regex::Regex;
use serde_json::json;
use std::sync::LazyLock;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("skull") };

static NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new("^[A-Za-z0-9_]+$").expect("regex"));
static URL_VALUE: LazyLock<Regex> = LazyLock::new(|| Regex::new("^[0-9a-fA-F]{64}$").expect("regex"));
static BASE_64: LazyLock<Regex> = LazyLock::new(|| Regex::new("^[A-Za-z0-9+/=]{180}$").expect("regex"));

/// The texture's hash, out of a `textures` value.
fn owner_of(encoded: &str) -> Option<String> {
    let decoded: serde_json::Value = serde_json::from_slice(&decode(encoded)?).ok()?;
    let url = decoded["textures"]["SKIN"]["url"].as_str()?;
    Some(url.rsplit('/').next()?.to_owned())
}

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let player = if args.len() == 2 { get_player_raw(None, args, 1, false, false)? } else { user.clone() };
    let owner = if let Some(first) = args.first().filter(|_| player.is_authorized("essentials.skull.others")) {
        if BASE_64.is_match(first) {
            let owner = owner_of(first).filter(|owner| URL_VALUE.is_match(owner));
            owner.ok_or_else(|| EssError::tl("skullInvalidBase64", &[]))?
        } else if NAME.is_match(first) {
            first.clone()
        } else {
            return Err(EssError::tl("alphaNames", &[]));
        }
    } else {
        user.get_name()
    };
    let is_self = *user == player;
    let held = user.get_item_in_hand().filter(|stack| stack.get_type().name() == "PLAYER_HEAD" && is_self);
    let spawn = held.is_none();
    let stack = if let Some(held) = held {
        held
    } else if user.is_authorized(if is_self { "essentials.skull.spawn" } else { "essentials.skull.spawn.others" }) {
        ItemStack::new(Material::of("PLAYER_HEAD"), 1)
    } else {
        return Err(EssError::tl("invalidSkull", &[]));
    };
    let mut meta = SkullMeta(stack);
    fail_if!(meta.has_owner() && !user.is_authorized("essentials.skull.modify"), EssError::tl("noPermissionSkull", &[]));
    let short = if URL_VALUE.is_match(&owner) {
        let texture = json!({ "textures": { "SKIN": { "url": format!("https://textures.minecraft.net/texture/{owner}") } } }).to_string();
        meta.0.set_component("minecraft:profile", json!({ "properties": [{ "name": "textures", "value": encode(texture.as_bytes()) }] }));
        owner[..7].to_owned()
    } else {
        meta.set_owner(&owner);
        owner
    };
    meta.0.set_display_name(&format!("\u{a7}fSkull of {short}"));
    if spawn {
        if let Some(base) = player.player() {
            inventories::add_item(&base, 0, false, std::slice::from_ref(&meta.0));
        }
        player.send_tl("givenSkull", crate::args![short.clone()]);
        if !is_self {
            user.send_tl("givenSkullOther", crate::args![player.get_display_name(), short]);
        }
        return Ok(());
    }
    if let Some(base) = user.player() {
        inventories::set_item_in_hand(&base, &meta.0);
    }
    user.send_tl("skullChanged", crate::args![short]);
    Ok(())
}

fn tab(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    if args.len() > 2 {
        return Some(Vec::new());
    }
    Some(if user.is_authorized("essentials.skull.others") { get_players(&user.get_source()) } else { vec![user.get_name()] })
}
