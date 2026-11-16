//! mcMMO's sidebar boards (Scoreboard.UseScoreboards): stats, a skill, top lists, ranks, cooldowns and inspect,
//! each for its Display_Time (or kept with `/mcscoreboard keep`), then back to the game's board.

use crate::locale::get as l;
use crate::mmo::{self, Out};
use crate::profile;
use crate::skills::Skill;
use horizon_bukkit::Player;
use horizon_bukkit::lead::{PlayerScoreboard, get_new_scoreboard};

pub fn enabled() -> bool {
    crate::settings::get().use_scoreboards
}

/// Whether the kind (`Stats`, `Top`...) prints to chat as well / at all.
pub fn prints(kind: &str) -> bool {
    let s = crate::settings::get();
    !s.use_scoreboards || s.board_time.get(kind).is_none_or(|t| t.0)
}

fn uses_board(kind: &str) -> bool {
    let s = crate::settings::get();
    s.use_scoreboards && s.board_time.get(kind).is_none_or(|t| t.1)
}

/// Shows `rows` (name, score) under `title` for the kind's display time.
pub fn show(player: &Player, kind: &str, title: &str, rows: &[(String, i32)]) {
    if !uses_board(kind) {
        return;
    }
    let s = crate::settings::get();
    let secs = s.board_time.get(kind).map(|t| t.2).unwrap_or(10);
    let board: PlayerScoreboard = get_new_scoreboard();
    let shown = horizon_bukkit::adventure::legacy(&horizon_kit::text::colorize(title));
    board.register_new_objective("mcmmo_sidebar", "dummy", &shown);
    board.set_display_slot("mcmmo_sidebar", "SIDEBAR");
    for (name, score) in rows.iter().take(15) {
        let mut entry = horizon_kit::text::colorize(name);
        if entry.len() > 40 {
            entry.truncate(40);
        }
        board.set_score("mcmmo_sidebar", &entry, *score);
    }
    player.set_scoreboard(&board);
    let uuid = player.get_unique_id().to_ascii_lowercase();
    let mut ss = mmo::sessions();
    if let Some(sess) = ss.get_mut(&uuid) {
        sess.board = Some(board);
        if !sess.board_keep {
            sess.board_until = profile::now_ms() + secs.max(1) * 1000;
        }
    }
}

/// The player's stats board.
pub fn stats(player: &Player, p: &profile::Profile, title: &str) {
    let mut rows: Vec<(String, i32)> = Skill::non_child().map(|s| (format!("&a{}", mmo::skill_name(s)), p.level[s.idx()])).collect();
    rows.push((l("Scoreboard.Misc.PowerLevel", &[]), p.power_level()));
    show(player, "Stats", title, &rows);
}

/// Boards whose time ran out taken down (a 1 s timer).
pub fn tick() {
    if !enabled() {
        return;
    }
    let now = profile::now_ms();
    let mut done: Vec<Player> = Vec::new();
    {
        let mut ss = mmo::sessions();
        for sess in ss.values_mut() {
            if sess.board.is_some() && !sess.board_keep && sess.board_until > 0 && sess.board_until <= now {
                sess.board = None;
                sess.board_until = 0;
                done.push(sess.player.clone());
            }
        }
    }
    for p in done {
        p.set_main_scoreboard();
    }
}

pub fn clear(player: &Player) -> bool {
    let uuid = player.get_unique_id().to_ascii_lowercase();
    let had = {
        let mut ss = mmo::sessions();
        ss.get_mut(&uuid).map(|s| {
            s.board_keep = false;
            s.board_until = 0;
            s.board.take().is_some()
        })
    };
    if had == Some(true) {
        player.set_main_scoreboard();
        return true;
    }
    false
}

/// A level changed while a board is up: nothing to redraw here (the boards are snapshots, as mcMMO's are
/// for every kind but the skill board).
pub fn level_changed(_out: &mut Out, _uuid: &str) {}
