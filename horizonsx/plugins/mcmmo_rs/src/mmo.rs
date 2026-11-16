//! Online players (mcMMO's McMMOPlayer): their session state, a permission snapshot, the XP pipeline (modifiers,
//! party share, level ups, caps, notifications, sounds, XP bars) and the super abilities' readying and activation.
//!
//! Locks: SESSIONS before STORE, and no game call that can fire an event while either is held: what a gain says
//! or plays is queued in an [`Out`] and done after the locks drop.

use crate::profile::{self, Profile, Store};
use crate::settings::{self, Settings};
use crate::skills::{ABILITIES, Ability, COUNT, Skill, Tool};
use horizon_bukkit::{BossBar, Player};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, MutexGuard};

/// Permission nodes read for each player at join and every few minutes (a check is a game query: never in a
/// hot handler).
pub mod perm {
    pub const SKILL0: usize = 0; // + skill index (19)
    pub const ABILITY0: usize = 19; // + ability index (8)
    pub const XP_QUAD: usize = 27;
    pub const XP_TRIPLE: usize = 28;
    pub const XP_150: usize = 29;
    pub const XP_DOUBLE: usize = 30;
    pub const XP_50: usize = 31;
    pub const XP_25: usize = 32;
    pub const XP_10: usize = 33;
    pub const XP_CUSTOM: usize = 34;
    pub const ACT_12: usize = 35;
    pub const ACT_8: usize = 36;
    pub const ACT_4: usize = 37;
    pub const CD_HALF: usize = 38;
    pub const CD_THIRD: usize = 39;
    pub const CD_QUARTER: usize = 40;
    pub const LUCKY: usize = 41;
    pub const ADMIN_CHAT: usize = 42;
    pub const CHAT_SPY: usize = 43;
    pub const BYPASS_HARDCORE: usize = 44;
    pub const PARTY: usize = 45;
    pub const PARTY_CHAT: usize = 46;
    pub const GREEN_THUMB: usize = 47;
    pub const COUNT: usize = 48;

    pub fn nodes() -> Vec<String> {
        let mut out = Vec::with_capacity(COUNT);
        for s in crate::skills::ALL {
            out.push(format!("mcmmo.skills.{}", s.lower()));
        }
        for a in crate::skills::ABILITIES {
            out.push(a.perm().to_owned());
        }
        for n in [
            "mcmmo.perks.xp.quadruple.all",
            "mcmmo.perks.xp.triple.all",
            "mcmmo.perks.xp.150percentboost.all",
            "mcmmo.perks.xp.double.all",
            "mcmmo.perks.xp.50percentboost.all",
            "mcmmo.perks.xp.25percentboost.all",
            "mcmmo.perks.xp.10percentboost.all",
            "mcmmo.perks.xp.customboost.all",
            "mcmmo.perks.activationtime.twelveseconds",
            "mcmmo.perks.activationtime.eightseconds",
            "mcmmo.perks.activationtime.fourseconds",
            "mcmmo.perks.cooldowns.halved",
            "mcmmo.perks.cooldowns.thirded",
            "mcmmo.perks.cooldowns.quartered",
            "mcmmo.perks.lucky.all",
            "mcmmo.chat.adminchat",
            "mcmmo.commands.mcchatspy",
            "mcmmo.bypass.hardcoremode",
            "mcmmo.commands.party",
            "mcmmo.chat.partychat",
            "mcmmo.ability.herbalism.greenthumb.all",
        ] {
            out.push(n.to_owned());
        }
        out
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Chat {
    None,
    Party,
    Admin,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Pve,
    Pvp,
    Gathering,
    Passive,
    Command,
    Shared,
}

pub struct Bar {
    pub bar: BossBar,
    pub until: i64,
    pub shown: bool,
}

pub struct Session {
    pub player: Player,
    pub uuid: String,
    pub name: String,
    pub perms: u64,
    pub active: [i64; 8],
    pub informed: [bool; 8],
    pub prepared: Option<(Tool, i64)>,
    pub ability_use: bool,
    pub notify: bool,
    pub levelup_sound: bool,
    pub god: bool,
    pub chat_spy: bool,
    pub debug: bool,
    pub chat: Chat,
    pub recently_hurt: i64,
    pub respawn_at: i64,
    pub teleport_at: i64,
    pub invite: Option<(String, String)>,
    pub ptp_request: Option<(String, i64)>,
    pub ptp_enabled: bool,
    pub ptp_confirm: bool,
    pub ptp_any: bool,
    pub bars: HashMap<Skill, Bar>,
    pub bar_dirty: [bool; COUNT],
    pub bar_hidden: [bool; COUNT],
    pub pos: Option<(String, f64, f64, f64)>,
    pub creative: bool,
    pub last_fish: Option<(f64, f64, f64, u32)>,
    pub repair_confirm: Option<(String, i64)>,
    pub salvage_confirm: Option<(String, i64)>,
    pub board_until: i64,
    pub board_keep: bool,
    pub board: Option<horizon_bukkit::lead::PlayerScoreboard>,
}

impl Session {
    pub fn new(player: &Player) -> Session {
        Session {
            player: player.clone(),
            uuid: player.get_unique_id().to_ascii_lowercase(),
            name: player.get_name().to_owned(),
            perms: u64::MAX,
            active: [0; 8],
            informed: [true; 8],
            prepared: None,
            ability_use: true,
            notify: true,
            levelup_sound: true,
            god: false,
            chat_spy: false,
            debug: false,
            chat: Chat::None,
            recently_hurt: 0,
            respawn_at: 0,
            teleport_at: 0,
            invite: None,
            ptp_request: None,
            ptp_enabled: true,
            ptp_confirm: true,
            ptp_any: false,
            bars: HashMap::new(),
            bar_dirty: [false; COUNT],
            bar_hidden: [false; COUNT],
            pos: None,
            creative: false,
            last_fish: None,
            repair_confirm: None,
            salvage_confirm: None,
            board_until: 0,
            board_keep: false,
            board: None,
        }
    }
    pub fn has(&self, node: usize) -> bool {
        self.perms & (1u64 << node) != 0
    }
    pub fn skill_perm(&self, skill: Skill) -> bool {
        self.has(perm::SKILL0 + skill.idx())
    }
    pub fn ability_active(&self, ab: Ability) -> bool {
        self.active[ab.idx()] > profile::now_ms()
    }
    pub fn any_active(&self) -> bool {
        let now = profile::now_ms();
        self.active.iter().any(|t| *t > now)
    }
    pub fn lucky(&self, skill: Skill) -> f64 {
        let _ = skill;
        if self.has(perm::LUCKY) { 1.333_333 } else { 1.0 }
    }
    pub fn xp_perk(&self) -> f64 {
        if self.has(perm::XP_CUSTOM) {
            return settings::get().custom_boost;
        }
        for (node, m) in [(perm::XP_QUAD, 4.0), (perm::XP_TRIPLE, 3.0), (perm::XP_150, 2.5), (perm::XP_DOUBLE, 2.0), (perm::XP_50, 1.5), (perm::XP_25, 1.25), (perm::XP_10, 1.1)] {
            if self.has(node) {
                return m;
            }
        }
        1.0
    }
    pub fn cooldown_perk(&self, seconds: i64) -> i64 {
        if self.has(perm::CD_HALF) {
            seconds / 2
        } else if self.has(perm::CD_THIRD) {
            seconds * 2 / 3
        } else if self.has(perm::CD_QUARTER) {
            seconds * 3 / 4
        } else {
            seconds
        }
    }
    pub fn activation_perk(&self, ticks: i64) -> i64 {
        if self.has(perm::ACT_12) {
            ticks + 12
        } else if self.has(perm::ACT_8) {
            ticks + 8
        } else if self.has(perm::ACT_4) {
            ticks + 4
        } else {
            ticks
        }
    }
}

pub static SESSIONS: LazyLock<Mutex<HashMap<String, Session>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn sessions() -> MutexGuard<'static, HashMap<String, Session>> {
    SESSIONS.lock().unwrap_or_else(|e| e.into_inner())
}

/// What to do once the locks are dropped.
#[derive(Default)]
pub struct Out(pub Vec<Box<dyn FnOnce() + Send>>);

impl Out {
    pub fn push(&mut self, f: impl FnOnce() + Send + 'static) {
        self.0.push(Box::new(f));
    }
    pub fn run(self) {
        for f in self.0 {
            f();
        }
    }
    pub fn tell(&mut self, p: &Player, text: String) {
        let p = p.clone();
        self.push(move || {
            for line in text.split('\n') {
                p.send_message(line);
            }
        });
    }
    /// A notification as advanced.yml's ActionBarNotifications says for its kind (action bar and/or chat).
    pub fn notify(&mut self, s: &Session, kind: &str, text: String) {
        if !s.notify || text.is_empty() {
            return;
        }
        let (bar, chat) = settings::get().notify(kind);
        let p = s.player.clone();
        self.push(move || {
            if bar {
                p.send_action_bar(&horizon_bukkit::adventure::legacy(&text));
            }
            if chat || !bar {
                p.send_message(&text);
            }
        });
    }
    pub fn sound(&mut self, p: &Player, kind: &str) {
        let s = settings::get();
        let Some(spec) = s.sounds.get(kind).cloned() else { return };
        if !spec.enabled {
            return;
        }
        let id = if spec.id.is_empty() { default_sound(kind).to_owned() } else { spec.id.clone() };
        let volume = spec.volume * s.master_volume;
        let p = p.clone();
        self.push(move || p.play_sound(&p.get_location(), &id, volume, spec.pitch));
    }
}

pub fn default_sound(kind: &str) -> &'static str {
    match kind {
        "ANVIL" => "BLOCK_ANVIL_PLACE",
        "ITEM_BREAK" => "ENTITY_ITEM_BREAK",
        "POP" => "ENTITY_ITEM_PICKUP",
        "CHIMAERA_WING" => "ENTITY_BAT_TAKEOFF",
        "LEVEL_UP" => "ENTITY_PLAYER_LEVELUP",
        "FIZZ" => "BLOCK_FIRE_EXTINGUISH",
        "TOOL_READY" => "ITEM_ARMOR_EQUIP_GOLD",
        "ROLL_ACTIVATED" => "ENTITY_LLAMA_SWAG",
        "SKILL_UNLOCKED" => "UI_TOAST_CHALLENGE_COMPLETE",
        "ABILITY_ACTIVATED_GENERIC" => "ITEM_TRIDENT_RIPTIDE_3",
        "BLEED" => "ENTITY_ENDER_EYE_DEATH",
        "GLASS" => "BLOCK_GLASS_BREAK",
        "ITEM_CONSUMED" => "ITEM_BOTTLE_EMPTY",
        "CRIPPLE" => "BLOCK_ANVIL_PLACE",
        _ => "ENTITY_EXPERIENCE_ORB_PICKUP",
    }
}

/// A skill's name as shown (Overhaul.Name.X).
pub fn skill_name(skill: Skill) -> String {
    crate::locale::get(&format!("Overhaul.Name.{}", skill.name()), &[])
}

// ---- Permission snapshots ----

static NODES: LazyLock<Vec<String>> = LazyLock::new(perm::nodes);

/// Reads the nodes for one player (server thread).
pub fn read_perms(p: &Player) -> u64 {
    let mut bits = 0u64;
    for (i, node) in NODES.iter().enumerate() {
        // Skills, abilities and party/green thumb nodes are mcMMO defaults (children of mcmmo.defaults): on
        // unless a permission plugin sets them false. Perks are off unless granted.
        let default_on = i < perm::XP_QUAD || matches!(i, perm::PARTY | perm::PARTY_CHAT | perm::GREEN_THUMB);
        if p.has_permission(node) || (default_on && !p.is_permission_set(node)) {
            bits |= 1 << i;
        }
    }
    bits
}

// ---- XP ----

/// The XP a level needs (cumulative curve: by power level).
pub fn xp_to_level(s: &Settings, p: &Profile, skill: Skill) -> i64 {
    let level = if s.cumulative { p.power_level() } else { p.level[skill.idx()] };
    s.xp_to_next(level)
}

fn power_capped(s: &Settings, p: &Profile) -> bool {
    s.power_cap > 0 && p.power_level() >= s.power_cap
}

/// The whole gain of `xp` (before modifiers) by the player `uuid`: what `beginXpGain` does.
pub fn gain(sessions: &mut HashMap<String, Session>, store: &mut Store, out: &mut Out, uuid: &str, skill: Skill, xp: f32, reason: Reason) {
    if xp <= 0.0 {
        return;
    }
    if skill.is_child() {
        let parents = skill.parents();
        let split = xp / parents.len() as f32;
        for parent in parents {
            if sessions.get(uuid).is_some_and(|s| s.skill_perm(*parent)) {
                gain(sessions, store, out, uuid, *parent, split, reason);
            }
        }
        return;
    }
    // Party XP share (EQUAL): split among the members near, with the bonus.
    if reason != Reason::Command
        && let Some(members) = crate::party::share_targets(sessions, uuid)
    {
        let s = settings::get();
        let size = members.len() as f64;
        let bonus = (s.party_share_base + size * s.party_share_increase).min(s.party_share_cap);
        let split = (xp as f64 / size * bonus) as f32;
        for m in members {
            unshared(sessions, store, out, &m, skill, split, if m == uuid { reason } else { Reason::Shared });
        }
        return;
    }
    unshared(sessions, store, out, uuid, skill, xp, reason);
}

/// `beginUnsharedXpGain`: the modifiers, then the gain, then the party's own XP.
pub fn unshared(sessions: &mut HashMap<String, Session>, store: &mut Store, out: &mut Out, uuid: &str, skill: Skill, xp: f32, reason: Reason) {
    let s = settings::get();
    let Some(sess) = sessions.get(uuid) else { return };
    if sess.creative && reason != Reason::Command {
        return;
    }
    let Some(p) = store.get(uuid) else { return };
    if p.level[skill.idx()] >= s.cap(skill) || power_capped(&s, p) {
        return;
    }
    let mut modified = xp;
    if reason != Reason::Command {
        modified = (xp as f64 * s.skill_mult[skill.idx()] * crate::xprate::rate(skill) * sess.xp_perk()) as f32;
    }
    apply(sessions, store, out, &s, uuid, skill, modified, reason);
    if reason != Reason::Command && reason != Reason::Shared {
        crate::party::party_xp(sessions, out, uuid, modified);
    }
}

/// `applyXpGain` + `checkXp`: the XP added, levels gained, everything said.
pub fn apply(sessions: &mut HashMap<String, Session>, store: &mut Store, out: &mut Out, s: &Settings, uuid: &str, skill: Skill, mut xp: f32, reason: Reason) {
    let Some(sess) = sessions.get_mut(uuid) else { return };
    if !sess.skill_perm(skill) {
        return;
    }
    let Some(p) = store.get_mut(uuid) else { return };
    if s.early_boost && reason != Reason::Command && p.level[skill.idx()] < 1 {
        xp += (xp_to_level(s, p, skill) as f64 * 0.05) as f32;
    }
    p.xp[skill.idx()] += xp;
    let cap = s.cap(skill);
    if p.level[skill.idx()] >= cap {
        p.xp[skill.idx()] = 0.0;
        return;
    }
    let mut gained = 0;
    while p.xp[skill.idx()] as f64 >= xp_to_level(s, p, skill) as f64 {
        if p.level[skill.idx()] >= cap || power_capped(s, p) {
            p.xp[skill.idx()] = 0.0;
            break;
        }
        let need = xp_to_level(s, p, skill) as f32;
        p.xp[skill.idx()] -= need;
        p.level[skill.idx()] += 1;
        gained += 1;
    }
    if s.bars_enabled && s.bars[skill.idx()].enabled && !sess.bar_hidden[skill.idx()] && (reason != Reason::Shared || s.bars_party) && (reason != Reason::Passive || s.bars_passive) {
        sess.bar_dirty[skill.idx()] = true;
    }
    if gained == 0 {
        return;
    }
    let level = p.level[skill.idx()];
    let power = p.power_level();
    let name = sess.name.clone();
    let player = sess.player.clone();
    if s.level_up_sounds && sess.levelup_sound {
        out.sound(&player, "LEVEL_UP");
    }
    out.notify(sess, "LevelUps", crate::locale::get("Overhaul.Levelup", &[&skill_name(skill), &gained.to_string(), &level.to_string()]));
    // Sub-skill ranks unlocked by these levels.
    let before = level - gained;
    for (key, levels) in s.ranks.iter() {
        if !key.starts_with(skill.name()) || !key[skill.name().len()..].starts_with('.') {
            continue;
        }
        for (i, l) in levels.iter().enumerate() {
            if *l > before && *l <= level && *l > 0 {
                let sub = &key[skill.name().len() + 1..];
                let sub_name = crate::locale::get(&format!("{}.SubSkill.{sub}.Name", skill.name()), &[]);
                out.notify(sess, "SubSkillUnlocked", crate::locale::get("JSON.SkillUnlockMessage", &[&sub_name, &(i + 1).to_string()]));
                out.sound(&player, "SKILL_UNLOCKED");
            }
        }
    }
    if level >= cap {
        out.tell(&player, crate::locale::get("LevelCap.Skill", &[&cap.to_string(), &skill_name(skill)]));
    }
    if s.power_cap > 0 && power >= s.power_cap {
        out.tell(&player, crate::locale::get("LevelCap.PowerLevel", &[&s.power_cap.to_string()]));
    }
    // Milestone broadcasts.
    if s.broadcast_levelups && (before / s.broadcast_interval) != (level / s.broadcast_interval) {
        let text = crate::locale::get("Broadcasts.LevelUpMilestone", &[&name, &(level / s.broadcast_interval * s.broadcast_interval).to_string(), &skill_name(skill)]);
        broadcast_near(out, s, &player, text);
    }
    let power_before = power - gained;
    if s.broadcast_power && (power_before / s.broadcast_power_interval) != (power / s.broadcast_power_interval) {
        let text = crate::locale::get("Broadcasts.PowerLevelUpMilestone", &[&name, &(power / s.broadcast_power_interval * s.broadcast_power_interval).to_string()]);
        broadcast_near(out, s, &player, text);
    }
    // level_up_commands.yml
    for c in &s.level_up_commands {
        let skill_hit = c.skills.contains(&skill) && c.levels.iter().any(|l| *l > before && *l <= level);
        let power_hit = c.power && c.levels.iter().any(|l| *l > power_before && *l <= power);
        if !skill_hit && !power_hit {
            continue;
        }
        for cmd in &c.commands {
            let line = cmd.replace("{@player}", &name).replace("{@level}", &level.to_string()).replace("{@skill}", &skill_name(skill)).replace("{@power_level}", &power.to_string()).replace(&format!("{{@{}_level}}", skill.lower()), &level.to_string());
            let as_player = c.as_player;
            let pl = player.clone();
            out.push(move || {
                if as_player {
                    pl.perform_command(&line);
                } else {
                    horizon_bukkit::fam_core::dispatch_command_as(&horizon_bukkit::CommandSender::Console, &line);
                }
            });
        }
    }
    if level > before {
        crate::scoreboard::level_changed(out, uuid);
    }
}

fn broadcast_near(out: &mut Out, s: &Settings, from: &Player, text: String) {
    let radius = s.broadcast_radius;
    let same_world = s.broadcast_same_world;
    let from = from.clone();
    out.push(move || {
        let here = from.get_location();
        for p in horizon_bukkit::get_online_players() {
            if same_world || radius.is_some() {
                let there = p.get_location();
                if there.get_world().get_name() != here.get_world().get_name() {
                    continue;
                }
                if let Some(r) = radius
                    && there.distance(&here) > r
                {
                    continue;
                }
            }
            p.send_message(&text);
        }
        crate::log(&horizon_kit::text::strip(&text));
    });
}

/// One gain from a handler: the locks taken, the gain made, the messages sent.
pub fn award(uuid: &str, skill: Skill, xp: f32, reason: Reason) {
    let mut out = Out::default();
    {
        let mut ss = sessions();
        let mut st = profile::store();
        gain(&mut ss, &mut st, &mut out, uuid, skill, xp, reason);
    }
    out.run();
}

// ---- Super abilities ----

/// Seconds left on a cooldown (<= 0: ready).
pub fn cooldown_left(sess: &Session, p: &Profile, ab: Ability) -> i64 {
    let s = settings::get();
    let dats = p.dats[profile::dats_slot(ab)];
    dats + sess.cooldown_perk(s.ability_cooldown(ab)) - profile::now_secs()
}

/// Right-click with a tool: readies it (`processAbilityActivation`).
pub fn ready_tool(uuid: &str, held: &str, sneaking: bool) {
    let s = settings::get();
    if !s.abilities_enabled || (s.only_sneak && !sneaking) {
        return;
    }
    let Some(tool) = Tool::of(held) else { return };
    let skill = match tool {
        Tool::Pickaxe => Skill::Mining,
        Tool::Axe => Skill::Woodcutting,
        Tool::Shovel => Skill::Excavation,
        Tool::Hoe => Skill::Herbalism,
        Tool::Sword => Skill::Swords,
        Tool::Fists => Skill::Unarmed,
        _ => return,
    };
    let mut out = Out::default();
    {
        let mut ss = sessions();
        let st = profile::store();
        let Some(sess) = ss.get_mut(uuid) else { return };
        let Some(p) = st.get(uuid) else { return };
        if !sess.ability_use || !sess.skill_perm(skill) || sess.any_active() {
            return;
        }
        if sess.prepared.is_some_and(|(t, until)| t == tool && until > profile::now_ms()) {
            return;
        }
        let Some(ab) = skill.super_ability() else { return };
        if tool != Tool::Axe {
            let left = cooldown_left(sess, p, ab);
            if left > 0 {
                out.notify(sess, "AbilityCoolDown", crate::locale::get("Skills.TooTired", &[&left.to_string()]));
                drop(st);
                drop(ss);
                out.run();
                return;
            }
        }
        if s.ability_messages {
            let text = if tool == Tool::Axe {
                axe_ready_text(sess, p)
            } else {
                crate::locale::get(&format!("{}.Ready", tool.locale()), &[])
            };
            out.notify(sess, "ToolReady", text);
            let pl = sess.player.clone();
            out.sound(&pl, "TOOL_READY");
        }
        sess.prepared = Some((tool, profile::now_ms() + 4000));
    }
    out.run();
}

fn axe_ready_text(sess: &Session, p: &Profile) -> String {
    let tf = cooldown_left(sess, p, Ability::TreeFeller);
    let ss = cooldown_left(sess, p, Ability::SkullSplitter);
    if tf > 0 {
        return crate::locale::get("Axes.Ability.Ready.Extra", &[&Ability::TreeFeller.display(), &tf.to_string()]);
    }
    if ss > 0 {
        return crate::locale::get("Axes.Ability.Ready.Extra", &[&Ability::SkullSplitter.display(), &ss.to_string()]);
    }
    crate::locale::get("Axes.Ability.Ready", &[])
}

/// A readied tool used on a block or a mob: the skill's super ability starts (`checkAbilityActivation`).
/// Returns whether it is (now or already) active.
pub fn try_activate(uuid: &str, skill: Skill, tool: Tool) -> bool {
    let Some(ab) = skill.super_ability() else { return false };
    let s = settings::get();
    let mut out = Out::default();
    let active = {
        let mut ss = sessions();
        let mut st = profile::store();
        let Some(sess) = ss.get_mut(uuid) else { return false };
        if sess.ability_active(ab) {
            return true;
        }
        let ready = sess.prepared.is_some_and(|(t, until)| t == tool && until > profile::now_ms());
        if !ready {
            return false;
        }
        if !sess.has(perm::ABILITY0 + ab.idx()) {
            return false;
        }
        let Some(p) = st.get(uuid) else { return false };
        let level = p.level(skill);
        let need = s.unlock_level(ab.rank_key());
        if level < need {
            out.notify(sess, "RequirementsNotMet", crate::locale::get("Skills.AbilityGateRequirementFail", &[&(need - level).to_string(), &skill_name(skill)]));
            sess.prepared = None;
            drop(st);
            drop(ss);
            out.run();
            return false;
        }
        let left = cooldown_left(sess, p, ab);
        if left > 0 {
            if matches!(skill, Skill::Woodcutting | Skill::Axes) {
                out.notify(sess, "AbilityCoolDown", crate::locale::get("Skills.TooTired", &[&left.to_string()]));
            }
            sess.prepared = None;
            drop(st);
            drop(ss);
            out.run();
            return false;
        }
        let capped = if s.ability_length_cap > 0 { level.min(s.ability_length_cap) } else { level };
        let mut secs = 2 + (capped / s.ability_length_step) as i64;
        let max = s.max_seconds[ab.idx()];
        if max > 0 {
            secs = secs.min(max);
        }
        secs = sess.activation_perk(secs);
        let end_ms = profile::now_ms() + secs * 1000;
        sess.active[ab.idx()] = end_ms;
        sess.informed[ab.idx()] = false;
        sess.prepared = None;
        if let Some(p) = st.get_mut(uuid) {
            p.dats[profile::dats_slot(ab)] = end_ms / 1000;
        }
        out.notify(sess, "SuperAbilityInteraction", crate::locale::get(&format!("{}.On", ab.locale()), &[]));
        let pl = sess.player.clone();
        let name = sess.name.clone();
        if s.notify_others {
            let text = crate::locale::get(&format!("{}.Other.On", ab.locale()), &[&name]);
            let me = pl.clone();
            out.push(move || {
                let here = me.get_location();
                for other in horizon_bukkit::get_online_players() {
                    if other.get_name() != me.get_name() && other.get_world().get_name() == here.get_world().get_name() && other.get_location().distance(&here) < 64.0 {
                        other.send_message(&text);
                    }
                }
            });
        }
        out.sound(&pl, "ABILITY_ACTIVATED_GENERIC");
        if matches!(ab, Ability::SuperBreaker | Ability::GigaDrillBreaker) {
            // mcMMO buffs the tool's Efficiency; here the same speed as Haste for the ability's length.
            let ticks = (secs * 20) as i32;
            let me = pl.clone();
            out.push(move || {
                me.give_potion_effect(&horizon_bukkit::PotionEffect::new("minecraft:haste", ticks, 4, false, false));
            });
        }
        true
    };
    out.run();
    active
}

/// Ends abilities whose time is up and lowers tools left readied (a 1 s timer).
pub fn tick_abilities() {
    let now = profile::now_ms();
    let mut out = Out::default();
    {
        let mut ss = sessions();
        let st = profile::store();
        let s = settings::get();
        for sess in ss.values_mut() {
            for ab in ABILITIES {
                if !sess.informed[ab.idx()] && sess.active[ab.idx()] <= now {
                    sess.informed[ab.idx()] = true;
                    out.notify(sess, "AbilityOff", crate::locale::get(&format!("{}.Off", ab.locale()), &[]));
                    if s.notify_others {
                        let text = crate::locale::get(&format!("{}.Other.Off", ab.locale()), &[&sess.name]);
                        let me = sess.player.clone();
                        out.push(move || {
                            let here = me.get_location();
                            for other in horizon_bukkit::get_online_players() {
                                if other.get_name() != me.get_name() && other.get_world().get_name() == here.get_world().get_name() && other.get_location().distance(&here) < 64.0 {
                                    other.send_message(&text);
                                }
                            }
                        });
                    }
                }
                // "Ability refreshed" once the cooldown ends.
                if sess.informed[ab.idx()]
                    && let Some(p) = st.get(&sess.uuid)
                {
                    let left = cooldown_left(sess, p, ab);
                    if left == 0 && p.dats[profile::dats_slot(ab)] > 0 {
                        out.notify(sess, "AbilityRefreshed", crate::locale::get(&format!("{}.Refresh", ab.locale()), &[]));
                    }
                }
            }
            if let Some((tool, until)) = sess.prepared
                && until <= now
            {
                sess.prepared = None;
                if s.ability_messages {
                    out.notify(sess, "ToolReady", crate::locale::get(&format!("{}.Lower", tool.locale()), &[]));
                }
            }
        }
    }
    out.run();
}

// ---- XP bars ----

/// Shows the bars of the skills that gained XP (a few-tick timer: one engine call per changed bar).
pub fn flush_bars() {
    let now = profile::now_ms();
    let s = settings::get();
    let mut work: Vec<(Player, Skill, Option<BossBar>, String, f64, bool)> = Vec::new();
    let mut hide: Vec<BossBar> = Vec::new();
    {
        let mut ss = sessions();
        let st = profile::store();
        for sess in ss.values_mut() {
            for skill in crate::skills::ALL {
                if !std::mem::take(&mut sess.bar_dirty[skill.idx()]) {
                    continue;
                }
                let Some(p) = st.get(&sess.uuid) else { continue };
                let level = p.level[skill.idx()];
                let need = xp_to_level(&s, p, skill).max(1);
                let progress = (p.xp[skill.idx()] as f64 / need as f64).clamp(0.0, 1.0);
                let title = if s.early_boost && level < 1 {
                    crate::locale::get("XPBar.Template.EarlyGameBoost", &[])
                } else {
                    crate::locale::get(&format!("XPBar.{}", skill.name()), &[&level.to_string()])
                };
                let existing = sess.bars.get(&skill).map(|b| b.bar.clone());
                let shown = sess.bars.get(&skill).is_some_and(|b| b.shown);
                work.push((sess.player.clone(), skill, existing, title, progress, shown));
            }
            for bar in sess.bars.values_mut() {
                if bar.shown && bar.until <= now {
                    bar.shown = false;
                    hide.push(bar.bar.clone());
                }
            }
        }
    }
    for bar in hide {
        bar.set_visible(false);
    }
    let mut made: Vec<(String, Skill, BossBar)> = Vec::new();
    for (player, skill, existing, title, progress, shown) in work {
        let bar = match existing {
            Some(b) => {
                b.set_title(&title);
                b.set_progress(progress);
                if !shown {
                    b.set_visible(true);
                }
                b
            }
            None => {
                let cfg = &s.bars[skill.idx()];
                let b = BossBar::create(&title, &cfg.color);
                b.set_style(&cfg.style);
                b.set_progress(progress);
                b.add_player(&player);
                b
            }
        };
        made.push((player.get_unique_id().to_ascii_lowercase(), skill, bar));
    }
    if made.is_empty() {
        return;
    }
    let mut ss = sessions();
    for (uuid, skill, bar) in made {
        if let Some(sess) = ss.get_mut(&uuid) {
            sess.bars.insert(skill, Bar { bar, until: now + 3000, shown: true });
        }
    }
}

/// A leaving player's bars taken down.
pub fn drop_bars(sess: &Session) {
    for b in sess.bars.values() {
        b.bar.remove_all();
    }
}
