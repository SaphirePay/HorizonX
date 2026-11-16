//! `Commanditemname`.
use crate::prelude::*;
use crate::utils::format::unformat_string_for;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("itemname") };

const PERM_PREFIX: &str = "essentials.itemname.prevent-type.";

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let Some(mut item) = user.get_item_in_hand().filter(|item| !item.get_type().is_air()) else {
        user.send_tl("itemnameInvalidItem", &[]);
        return Ok(());
    };
    let wildcard = user.is_authorized_exact(&format!("{PERM_PREFIX}*"));
    let material = user.is_authorized_exact(&format!("{PERM_PREFIX}{}", item.get_type().name().to_lowercase()));
    if (wildcard == Some(true) && material != Some(false)) || (wildcard != Some(true) && material == Some(true)) {
        user.send_tl("itemnameInvalidItem", &[]);
        return Ok(());
    }
    let name = format_string(user, "essentials.itemname", &get_final_arg(args, 0)).trim().to_owned();
    if name.is_empty() {
        item.remove_component("minecraft:custom_name");
    } else {
        item.set_display_name(&name);
    }
    if let Some(player) = user.player() {
        inventories::set_item_in_hand(&player, &item);
    }
    if name.is_empty() {
        user.send_tl("itemnameClear", &[]);
    } else {
        user.send_tl("itemnameSuccess", crate::args![name]);
    }
    Ok(())
}

fn tab(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    if args.len() == 1
        && let Some(name) = user.get_item_in_hand().filter(|item| !item.get_type().is_air()).and_then(|item| item.get_display_name())
    {
        return Some(vec![unformat_string_for(user, "essentials.itemname", &name)]);
    }
    Some(Vec::new())
}
