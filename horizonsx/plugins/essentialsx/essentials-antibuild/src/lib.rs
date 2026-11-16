//! `EssentialsAntiBuild`. Its settings are read each time from `config.yml`, which `Settings` holds.
use essentials::prelude::*;
use horizon_bukkit::fam_ev_world::BlockPistonEvent;
use essentials::utils::material::{is_hanging_sign, is_sign_post, is_wall_hanging_sign, is_wall_sign};
use horizon_bukkit::enums::event_priority::{HIGH, HIGHEST, LOW};
use horizon_bukkit::event::{BlockBreakEvent, PlayerInteractEvent};
use horizon_bukkit::event2::PlayerInteractEntityEvent;
use horizon_bukkit::events::{BlockPistonExtendEvent, BlockPistonRetractEvent, EntityPickupItemEvent, PlayerDropItemEvent};
use horizon_bukkit::fam_ev_entity::{Actor, EntityDamageByEntityEvent};
use horizon_bukkit::fam_ev_player::CraftItemEvent;
use horizon_bukkit::fam_ev_world::{BlockDispenseEvent, BlockPlaceEvent, HangingBreakByEntityEvent, HangingEvent};
use horizon_bukkit::{Block, Plugin};

const DISABLE_BUILD: (&str, bool) = ("protect.disable.build", true);
const DISABLE_USE: (&str, bool) = ("protect.disable.use", true);
const ALERT_ON_PLACEMENT: &str = "protect.alert.on-placement";
const ALERT_ON_USE: &str = "protect.alert.on-use";
const ALERT_ON_BREAK: &str = "protect.alert.on-break";
const BLACKLIST_PLACEMENT: &str = "protect.blacklist.placement";
const BLACKLIST_USAGE: &str = "protect.blacklist.usage";
const BLACKLIST_BREAK: &str = "protect.blacklist.break";
const BLACKLIST_PISTON: &str = "protect.blacklist.piston";
const BLACKLIST_DISPENSER: &str = "protect.blacklist.dispenser";

fn get_setting_bool(setting: (&str, bool)) -> bool {
    settings().get_protect_boolean(setting.0, setting.1)
}

fn check_protection_items(list: &str, material: &Material) -> bool {
    settings().get_protect_list(list).contains(material)
}

/// `getNameForType`.
fn name_for(material: &Material) -> String {
    material.name().to_lowercase().replace('_', " ")
}

fn meta_perm_check(user: &User, action: &str, material: &Material) -> bool {
    user.is_authorized(&format!("essentials.build.{action}.{}", material.name()))
}

fn warn(user: &User, key: &str, item: String) {
    if settings().warn_on_build_disallow() {
        user.send_tl(key, essentials::args![item]);
    }
}

/// `EssentialsConnect.alert`.
fn alert(user: &User, item: &str, key: &str) {
    let Some(location) = user.get_location() else { return };
    let place = format!("{},{},{},{}", location.world.get_name(), location.get_block_x(), location.get_block_y(), location.get_block_z());
    essentials::warning(&mini_to_legacy(&tl_literal("alertFormat", essentials::args![user.get_name(), tl_literal(key, &[]), item, place.clone()])));
    for other in ess().get_online_users() {
        if other.is_authorized("essentials.protect.alerts") {
            other.send_tl("alertFormat", essentials::args![user.get_name(), other.player_tl(key, &[]), item, place.clone()]);
        }
    }
}

fn would_break_attached_signs(block: &Block, checker: impl Fn(&Block) -> bool) -> bool {
    let above = block.get_relative(0, 1, 0);
    if is_sign_post(&above.get_type()) && checker(&above) {
        return true;
    }
    let below = block.get_relative(0, -1, 0);
    if is_hanging_sign(&below.get_type()) && checker(&below) {
        return true;
    }
    [(0, -1, "north"), (1, 0, "east"), (0, 1, "south"), (-1, 0, "west")].iter().any(|(x, z, face)| {
        let sign = block.get_relative(*x, 0, *z);
        let material = sign.get_type();
        (is_wall_sign(&material) || is_wall_hanging_sign(&material)) && sign.get_property("facing").as_deref() == Some(*face) && checker(&sign)
    })
}

fn is_sign_protected(sign: &Block, user: &User) -> bool {
    let kind = sign.get_type();
    (get_setting_bool(DISABLE_BUILD) && !builds(&user) && !meta_perm_check(user, "break", &kind)) || (check_protection_items(BLACKLIST_BREAK, &kind) && !user.is_authorized("essentials.protect.exemptbreak"))
}

/// `!user.canBuild() && !user.isAuthorized("essentials.build")` in every AntiBuild check (EssentialsX): a
/// permissions plugin grants building with `essentials.build`; `canBuild` alone is op-only here.
fn builds(user: &User) -> bool {
    user.can_build() || user.is_authorized("essentials.build")
}

fn user_of(player: &Player) -> Option<User> {
    ess().get_user(player)
}

/// The blocked-or-alerted steps both the entity handlers share, for `name`.
fn place_entity(user: &User, material: &Material, name: &str) -> bool {
    if get_setting_bool(DISABLE_BUILD) && !builds(user) && !meta_perm_check(user, "place", material) {
        warn(user, "antiBuildPlace", name.to_owned());
        return true;
    }
    if check_protection_items(BLACKLIST_PLACEMENT, material) && !user.is_authorized("essentials.protect.exemptplacement") {
        warn(user, "antiBuildPlace", name.to_owned());
        return true;
    }
    if check_protection_items(ALERT_ON_PLACEMENT, material) && !user.is_authorized("essentials.protect.alerts.notrigger") {
        alert(user, name, "alertPlaced");
    }
    false
}

/// `event.getDamager()`, or the player who shot it.
fn attacker(event: &EntityDamageByEntityEvent) -> Option<Player> {
    match event.get_damager()? {
        Actor::Player(player) => Some(player),
        Actor::Entity(entity) => {
            let owner = entity.data_at("Owner");
            let parts: Vec<i64> = owner.as_array()?.iter().filter_map(serde_json::Value::as_i64).collect();
            let hex: String = parts.iter().map(|part| format!("{:08x}", *part as i32)).collect();
            horizon_bukkit::fam_ess::get_player_by_id(&hex)
        }
    }
}

/// `onBlockEntityDamage`: `true` cancels.
fn entity_damage(event: &EntityDamageByEntityEvent) -> bool {
    let Some(user) = attacker(event).and_then(|player| user_of(&player)) else { return false };
    let Some(Actor::Entity(target)) = event.get_entity() else { return false };
    let name = match target.get_type().trim_start_matches("minecraft:").to_uppercase().as_str() {
        "ITEM_FRAME" | "GLOW_ITEM_FRAME" => "ITEM_FRAME",
        "ARMOR_STAND" => "ARMOR_STAND",
        "END_CRYSTAL" => "END_CRYSTAL",
        _ => return false,
    };
    let material = Material::of(name);
    if get_setting_bool(DISABLE_BUILD) && !builds(&user) && !meta_perm_check(&user, "break", &material) {
        warn(&user, "antiBuildBreak", name.to_owned());
        return true;
    }
    if check_protection_items(BLACKLIST_BREAK, &material) && !user.is_authorized("essentials.protect.exemptbreak") {
        warn(&user, "antiBuildBreak", name.to_owned());
        return true;
    }
    if check_protection_items(ALERT_ON_BREAK, &material) && !user.is_authorized("essentials.protect.alerts.notrigger") {
        alert(&user, name, "alertBroke");
    }
    false
}

fn blocked_push(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| check_protection_items(BLACKLIST_PISTON, &block.get_type()) || would_break_attached_signs(block, |_| true))
}

/// `onPlayerInteract`.
fn player_interact(event: &mut PlayerInteractEvent) {
    let player = event.get_player().clone();
    let Some(user) = user_of(&player) else { return };
    let item = inventories::get_item_in_hand(&player).filter(|item| !item.get_type().is_air());
    if let Some(item) = &item {
        let kind = item.get_type();
        if check_protection_items(BLACKLIST_USAGE, kind) && !user.is_authorized("essentials.protect.exemptusage") {
            warn(&user, "antiBuildUse", kind.name());
            event.set_cancelled(true);
            return;
        }
        if check_protection_items(ALERT_ON_USE, kind) && !user.is_authorized("essentials.protect.alerts.notrigger") {
            alert(&user, &kind.name(), "alertUsed");
        }
    }
    if !get_setting_bool(DISABLE_USE) || builds(&user) {
        return;
    }
    if let Some(item) = item.filter(|item| !meta_perm_check(&user, "interact", item.get_type())) {
        event.set_cancelled(true);
        warn(&user, "antiBuildUse", item.get_type().name());
        return;
    }
    if let Some(block) = event.get_clicked_block().cloned()
        && !meta_perm_check(&user, "interact", &block.get_type())
    {
        event.set_cancelled(true);
        warn(&user, "antiBuildInteract", block.get_type().name());
    }
}

/// `onEnable`.
pub fn on_enable(plugin: Plugin) {
    plugin.register_event_with(HIGHEST, true, |event: &mut BlockPlaceEvent| {
        let Some(user) = user_of(event.get_player()) else { return };
        let kind = event.get_block_placed().get_type();
        if kind == event.get_block_replaced_state().get_type() {
            return;
        }
        if (get_setting_bool(DISABLE_BUILD) && !builds(&user) && !meta_perm_check(&user, "place", &kind)) || (check_protection_items(BLACKLIST_PLACEMENT, &kind) && !user.is_authorized("essentials.protect.exemptplacement")) {
            warn(&user, "antiBuildPlace", name_for(&kind));
            event.set_cancelled(true);
            return;
        }
        if check_protection_items(ALERT_ON_PLACEMENT, &kind) && !user.is_authorized("essentials.protect.alerts.notrigger") {
            alert(&user, &name_for(&kind), "alertPlaced");
        }
    });
    plugin.register_event_with(HIGHEST, true, |event: &mut BlockBreakEvent| {
        let Some(user) = user_of(event.get_player()) else { return };
        let block = event.get_block().clone();
        let kind = block.get_type();
        let refused = would_break_attached_signs(&block, |sign| is_sign_protected(sign, &user)) || (get_setting_bool(DISABLE_BUILD) && !builds(&user) && !meta_perm_check(&user, "break", &kind)) || (check_protection_items(BLACKLIST_BREAK, &kind) && !user.is_authorized("essentials.protect.exemptbreak"));
        if refused {
            warn(&user, "antiBuildBreak", name_for(&kind));
            event.set_cancelled(true);
            return;
        }
        if check_protection_items(ALERT_ON_BREAK, &kind) && !user.is_authorized("essentials.protect.alerts.notrigger") {
            alert(&user, &name_for(&kind), "alertBroke");
        }
    });
    plugin.register_event_with(HIGHEST, true, |event: &mut HangingBreakByEntityEvent| {
        let Some(user) = event.get_remover().and_then(|remover| horizon_bukkit::fam_ess::get_player_by_id(&remover.get_unique_id())).and_then(|player| user_of(&player)) else { return };
        if !get_setting_bool(DISABLE_BUILD) || builds(&user) {
            return;
        }
        let name = event.get_entity().map(|entity| entity.get_type().trim_start_matches("minecraft:").to_uppercase()).unwrap_or_default();
        if (name == "PAINTING" || name == "ITEM_FRAME") && !meta_perm_check(&user, "break", &Material::of(&name)) {
            warn(&user, "antiBuildBreak", name);
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(HIGH, true, |event: &mut PlayerInteractEntityEvent| {
        let Some(user) = user_of(event.get_player()) else { return };
        if let Some(hand) = inventories::get_item_in_main_hand(event.get_player())
            && hand.get_type().name() == "SHEARS"
            && get_setting_bool(DISABLE_USE)
            && !builds(&user)
            && !meta_perm_check(&user, "interact", hand.get_type())
        {
            warn(&user, "antiBuildUse", hand.get_type().name());
            event.set_cancelled(true);
            return;
        }
        let name = match event.get_entity_type().trim_start_matches("minecraft:") {
            "item_frame" | "glow_item_frame" => "ITEM_FRAME",
            "armor_stand" => "ARMOR_STAND",
            _ => return,
        };
        if place_entity(&user, &Material::of(name), name) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(HIGHEST, true, |event: &mut EntityDamageByEntityEvent| {
        if entity_damage(event) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(HIGHEST, true, |event: &mut BlockPistonExtendEvent| {
        if blocked_push(&event.get_blocks()) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(HIGHEST, true, |event: &mut BlockPistonRetractEvent| {
        if event.get_block().is_some_and(|base| base.get_type().name() == "STICKY_PISTON") && blocked_push(&event.get_blocks()) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(LOW, false, player_interact);
    plugin.register_event_with(LOW, true, |event: &mut CraftItemEvent| {
        let Some(user) = event.get_who_clicked().and_then(|player| user_of(&player)) else { return };
        let Some(item) = event.get_recipe().and_then(|recipe| recipe.get_result()) else { return };
        if get_setting_bool(DISABLE_USE) && !builds(&user) && !meta_perm_check(&user, "craft", item.get_type()) {
            event.set_cancelled(true);
            warn(&user, "antiBuildCraft", item.get_type().name());
        }
    });
    plugin.register_event_with(LOW, true, |event: &mut PlayerDropItemEvent| {
        let (Some(user), Some(item)) = (event.get_player().and_then(|player| user_of(&player)), event.get_item_drop()) else {
            return;
        };
        if get_setting_bool(DISABLE_USE) && !builds(&user) && !meta_perm_check(&user, "drop", item.get_type()) {
            event.set_cancelled(true);
            warn(&user, "antiBuildDrop", item.get_type().name());
        }
    });
    plugin.register_event_with(HIGHEST, true, |event: &mut BlockDispenseEvent| {
        if event.get_item().is_some_and(|item| check_protection_items(BLACKLIST_DISPENSER, item.get_type())) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(LOW, true, |event: &mut EntityPickupItemEvent| {
        let Some(Actor::Player(player)) = event.get_entity() else { return };
        let (Some(user), Some(item)) = (user_of(&player), event.get_item()) else {
            return;
        };
        if get_setting_bool(DISABLE_USE) && !builds(&user) && !meta_perm_check(&user, "pickup", item.get_type()) {
            event.set_cancelled(true);
        }
    });
}
