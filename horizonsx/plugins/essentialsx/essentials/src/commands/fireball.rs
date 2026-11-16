//! `Commandfireball`.
use crate::prelude::*;
use crate::mob_data::uuid_snbt;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("fireball") };

pub const FIREBALL_META_KEY: &str = "ess_fireball_proj";

const TYPES: [(&str, &str); 13] = [
    ("fireball", "fireball"),
    ("small", "small_fireball"),
    ("large", "fireball"),
    ("arrow", "arrow"),
    ("skull", "wither_skull"),
    ("egg", "egg"),
    ("snowball", "snowball"),
    ("expbottle", "experience_bottle"),
    ("dragon", "dragon_fireball"),
    ("splashpotion", "splash_potion"),
    ("lingeringpotion", "lingering_potion"),
    ("trident", "trident"),
    ("windcharge", "wind_charge"),
];

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let kind = args.first().filter(|first| TYPES.iter().any(|(name, _)| name == first)).map_or("fireball", String::as_str);
    let ride = args.get(2).is_some_and(|third| third.eq_ignore_ascii_case("ride")) && user.is_authorized("essentials.fireball.ride");
    let mut speed = 2.0;
    if let Some(Ok(asked)) = args.get(1).map(|text| parse_double(text)) {
        speed = asked.min(settings().get_max_projectile_speed()).max(0.0);
    }
    let node = format!("essentials.fireball.{kind}");
    fail_if!(!user.is_authorized(&node), EssError::tl("noPerm", crate::args![node]));
    let Some(player) = user.player() else { return Ok(()) };
    let mut at = player.get_eye_location();
    let looking = at.get_direction();
    let (x, y, z) = (looking.x * speed, looking.y * speed, looking.z * speed);
    at.add(x, y, z);
    let entity = TYPES.iter().find(|(name, _)| *name == kind).map_or("fireball", |(_, entity)| entity);
    let Some(projectile) = at.world.spawn(&at, entity) else { return Ok(()) };
    projectile.merge(&format!("{{Owner:{}}}", uuid_snbt(&player.get_unique_id())));
    projectile.set_velocity(x, y, z);
    projectile.add_scoreboard_tag(FIREBALL_META_KEY);
    if ride {
        player.ride(&projectile);
    }
    Ok(())
}

fn tab(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => TYPES.iter().map(|(name, _)| *name).filter(|name| user.is_authorized(&format!("essentials.fireball.{name}"))).map(str::to_owned).collect(),
        2 => ["1", "2", "3", "4", "5"].map(str::to_owned).to_vec(),
        _ => Vec::new(),
    })
}
