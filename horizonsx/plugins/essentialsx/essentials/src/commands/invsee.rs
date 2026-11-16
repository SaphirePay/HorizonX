//! `Commandinvsee`.
use crate::prelude::*;
use horizon_bukkit::Inventory;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("invsee") };

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    let inv_user = get_player_for(user, args, 0)?;
    if inv_user == *user {
        user.send_tl("invseeNoSelf", &[]);
        return Err(EssError::NoCharge);
    }
    let (Some(player), Some(base)) = (user.player(), inv_user.player()) else {
        return Ok(());
    };
    let equipped = (args.len() > 1 && user.is_authorized("essentials.invsee.equip")).then(|| {
        let mut equipped = Inventory::create(9, &user.player_tl("equipped", &[]));
        for slot in 0..4 {
            equipped.set_item(slot as usize, inventories::get_item(&base, 36 + slot));
        }
        equipped.set_item(4, inventories::get_item(&base, 40));
        equipped
    });
    let (user, owner) = (user.clone(), inv_user.get_uuid());
    inventories::after_turn(move || {
        player.close_inventory();
        let opened = match &equipped {
            Some(equipped) => {
                player.open_inventory(equipped);
                true
            }
            None => player.open_player_inventory(&base.get_inventory()),
        };
        if opened {
            user.set_inv_see_owner(equipped.is_none().then_some(owner));
            user.set_inv_see(true);
        }
    });
    Ok(())
}

fn tab(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    if args.len() != 1 {
        return Some(Vec::new());
    }
    let mut suggestions = get_players(&user.get_source());
    let own = user.get_name();
    suggestions.retain(|name| *name != own);
    Some(suggestions)
}
