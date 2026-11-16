//! `craftbukkit/SetExpFix`: experience by Mojang's formulas, not what Bukkit's total remembers.
use horizon_bukkit::Player;

/// `getExpAtLevel(int)`.
pub fn get_exp_at_level(level: i32) -> i32 {
    if level <= 15 {
        2 * level + 7
    } else if level <= 30 {
        5 * level - 38
    } else {
        9 * level - 158
    }
}

/// `setTotalExperience`.
pub fn set_total_experience(player: &Player, exp: i32) {
    player.set_exp(0.0);
    player.set_level(0);
    player.set_total_experience(0);
    player.give_exp(exp.max(0));
}

/// `getTotalExperience`.
pub fn get_total_experience(player: &Player) -> i32 {
    total_from(player.get_level(), player.get_exp())
}

fn total_from(level: i32, progress: f32) -> i32 {
    let mut exp = i64::from((get_exp_at_level(level) as f32 * progress).round() as i32);
    for below in 0..level {
        exp += i64::from(get_exp_at_level(below));
    }
    i32::try_from(exp).unwrap_or(i32::MAX)
}

/// `getExpToLevel`.
pub fn get_exp_to_level(level: i32) -> i32 {
    let total: i64 = (0..level).map(|below| i64::from(get_exp_at_level(below))).sum();
    i32::try_from(total).unwrap_or(i32::MAX)
}

/// `getExpUntilNextLevel`.
pub fn get_exp_until_next_level(player: &Player) -> i32 {
    let level = player.get_level();
    get_exp_at_level(level) - (get_exp_at_level(level) as f32 * player.get_exp()).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_follow_the_formula() {
        assert_eq!(get_exp_to_level(1), 7);
        assert_eq!(get_exp_to_level(16), 352);
        assert_eq!(get_exp_to_level(31), 1507);
        assert_eq!(total_from(1, 0.5), 12);
    }
}
