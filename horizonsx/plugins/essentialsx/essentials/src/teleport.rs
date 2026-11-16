//! `AsyncTeleport` and `AsyncTimedTeleport`: cooldown, warmup, safe landing, and the charge. There's no
//! off-thread chunk load here, so what `getChunkAtAsync` waits for happens at once, on this thread.
use crate::prelude::*;
use crate::utils::location::{self, java_round};
use horizon_bukkit::scheduler;
use std::sync::{Arc, Mutex};

/// What `respawn` asks in place of its own `PlayerRespawnEvent`: EssentialsSpawn answers it.
pub static RESPAWN_HOOK: std::sync::OnceLock<fn(&User, Location) -> Location> = std::sync::OnceLock::new();

const MOVE_CONSTANT: f64 = 0.3;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TeleportType {
    Tpa,
    Back,
    #[default]
    Normal,
}

/// `ITarget`: a player's target is read at the moment it's needed.
#[derive(Clone, Debug)]
pub enum Target {
    Location(Location),
    Player(Player),
}

impl Target {
    pub fn get_location(&self) -> Location {
        match self {
            Self::Location(location) => location.clone(),
            Self::Player(player) => player.get_location(),
        }
    }
}

/// What a `User` keeps for its `AsyncTeleport`.
#[derive(Default)]
pub struct TeleportState {
    tp_type: TeleportType,
    timed: Option<Arc<TimedTeleport>>,
}

/// `getLastTeleportTimestamp` compared with `teleport-cooldown`.
#[derive(Clone)]
pub struct AsyncTeleport {
    owner: User,
}

fn cooldown_end(from: i64, cooldown: f64) -> i64 {
    from + i64::from(cooldown as i32) * 1000 + i64::from(((cooldown * 1000.0) % 1000.0) as i32)
}

pub fn teleporting_args(location: &Location) -> Vec<Arg> {
    vec![Arg::from(location.get_world().get_name()), Arg::from(location.get_block_x()), Arg::from(location.get_block_y()), Arg::from(location.get_block_z())]
}

impl AsyncTeleport {
    pub fn new(owner: User) -> Self {
        Self { owner }
    }
    fn state(&self) -> std::sync::MutexGuard<'_, TeleportState> {
        self.owner.teleport_state()
    }
    pub fn get_tp_type(&self) -> TeleportType {
        self.state().tp_type
    }
    pub fn set_tp_type(&self, kind: TeleportType) {
        self.state().tp_type = kind;
    }
    fn cooldown_applies(&self) -> bool {
        const GLOBAL: &str = "essentials.teleport.cooldown.bypass";
        match self.get_tp_type() {
            TeleportType::Normal => !self.owner.is_authorized(GLOBAL),
            TeleportType::Back => !(self.owner.is_authorized(GLOBAL) && self.owner.is_authorized("essentials.teleport.cooldown.bypass.back")),
            TeleportType::Tpa => !(self.owner.is_authorized(GLOBAL) && self.owner.is_authorized("essentials.teleport.cooldown.bypass.tpa")),
        }
    }
    /// `cooldown(check, future)`: true if it refused, with the refusal in the future.
    pub fn cooldown_future(&self, check: bool, future: &EssFuture) -> bool {
        let now = now_ms();
        let last = self.owner.get_last_teleport_timestamp();
        if last > 0 {
            let cooldown = settings().get_teleport_cooldown();
            let earliest = cooldown_end(now, -cooldown);
            if last > now {
                self.owner.set_last_teleport_timestamp(now);
                return false;
            } else if last > earliest && self.cooldown_applies() {
                future.complete_exceptionally(EssError::tl("timeBeforeTeleport", crate::args![format_date_diff(cooldown_end(last, cooldown))]));
                return true;
            }
        }
        if !check {
            self.owner.set_last_teleport_timestamp(now);
        }
        false
    }
    /// `cooldown(check)`.
    pub fn cooldown(&self, check: bool) -> EssResult {
        let future = EssFuture::new();
        if self.cooldown_future(check, &future)
            && let Some(error) = future.failure()
        {
            return Err(error);
        }
        Ok(())
    }
    fn warn_user(&self, user: &User, delay: f64) {
        user.send_tl("dontMoveMessage", crate::args![format_date_diff(cooldown_end(now_ms(), delay))]);
    }
    /// `now(Location, cooldown, cause, future)`.
    pub fn now(&self, location: &Location, cooldown: bool, cause: &'static str, future: EssFuture) {
        if cooldown && self.cooldown_future(false, &future) {
            return;
        }
        self.now_async(&self.owner.clone(), &Target::Location(location.clone()), cause, &future);
    }
    /// `now(Player, cooldown, cause, future)`.
    pub fn now_player(&self, entity: &Player, cooldown: bool, cause: &'static str, future: EssFuture) {
        if cooldown && self.cooldown_future(false, &future) {
            future.complete(false);
            return;
        }
        let target = Target::Player(entity.clone());
        self.now_async(&self.owner.clone(), &target, cause, &future);
        let owner = self.owner.clone();
        future.then_accept(move |success| {
            if success {
                owner.send_tl("teleporting", &teleporting_args(&target.get_location()));
            }
        });
    }
    /// `nowUnsafe`.
    pub fn now_unsafe(&self, location: &Location, future: EssFuture) {
        let done = self.owner.player().is_some_and(|player| player.teleport(location));
        future.complete(done);
    }
    /// `nowAsync`.
    pub(crate) fn now_async(&self, teleportee: &User, target: &Target, _cause: &'static str, future: &EssFuture) {
        self.cancel(false);
        let Some(player) = teleportee.player() else {
            future.complete(false);
            return;
        };
        if !settings().is_force_passenger_teleport() && player.has_passengers() {
            if !settings().is_teleport_passenger_dismount() {
                future.complete_exceptionally(EssError::tl("passengerTeleportFail", &[]));
                return;
            }
            player.eject();
        }
        if teleportee.is_authorized("essentials.back.onteleport") {
            teleportee.set_last_location();
        }
        let mut target_location = target.get_location();
        let world = target_location.get_world().clone();
        if settings().is_teleport_safety_enabled() && !settings().is_force_disable_teleport_safety() && location::is_block_outside_world_border(&world, target_location.get_block_x(), target_location.get_block_z()) {
            target_location.x = f64::from(location::get_x_inside_world_border(&world, target_location.get_block_x()));
            target_location.z = f64::from(location::get_z_inside_world_border(&world, target_location.get_block_z()));
        }
        let (user, done) = (teleportee.clone(), future.clone());
        let (chunk_x, chunk_z) = (target_location.get_block_x() >> 4, target_location.get_block_z() >> 4);
        // `PaperLib.getChunkAtAsync`: what's there can only be judged once it's in.
        crate::random_teleport::with_chunk(world, chunk_x, chunk_z, Box::new(move |_| land(&user, target_location, &done)));
    }
    /// `teleport(Location, chargeFor, cause, future)`.
    pub fn teleport(&self, location: &Location, charge_for: Option<&Trade>, cause: &'static str, future: EssFuture) {
        self.teleport_to(&self.owner.clone(), Target::Location(location.clone()), charge_for, cause, future);
    }
    /// `teleport(Player, chargeFor, cause, future)`.
    pub fn teleport_player_target(&self, entity: &Player, charge_for: Option<&Trade>, cause: &'static str, future: EssFuture) {
        self.owner.send_tl("teleportToPlayer", crate::args![entity.display_name().to_legacy()]);
        self.teleport_to(&self.owner.clone(), Target::Player(entity.clone()), charge_for, cause, future);
    }
    /// `teleportPlayer(otherUser, Location, ...)`.
    pub fn teleport_player(&self, other: &User, location: &Location, charge_for: Option<&Trade>, cause: &'static str, future: EssFuture) {
        self.teleport_to(other, Target::Location(location.clone()), charge_for, cause, future);
    }
    /// `teleportPlayer(otherUser, Player, ...)`.
    pub fn teleport_player_to(&self, other: &User, entity: &Player, charge_for: Option<&Trade>, cause: &'static str, future: EssFuture) {
        let target = Target::Player(entity.clone());
        self.teleport_to(other, target.clone(), charge_for, cause, future.clone());
        let (other, owner) = (other.clone(), self.owner.clone());
        future.then_accept(move |success| {
            if success {
                let place = teleporting_args(&target.get_location());
                other.send_tl("teleporting", &place);
                owner.send_tl("teleporting", &place);
            }
        });
    }
    fn converted_charge(charge_for: Option<&Trade>, payer: &User) -> Option<Trade> {
        let charge = charge_for?;
        let cost = charge.get_command_cost(payer);
        // Costs are checked in the world they were asked in, not the one landed in.
        if cost.equals_exact(&BigDecimal::zero()) { Some(charge.clone()) } else { Some(Trade::of_money(cost)) }
    }
    fn teleport_to(&self, teleportee: &User, target: Target, charge_for: Option<&Trade>, cause: &'static str, future: EssFuture) {
        let delay = settings().get_teleport_delay();
        if let Some(charge) = charge_for {
            charge.is_affordable_for_future(&self.owner, &future);
            if future.is_completed_exceptionally() {
                return;
            }
        }
        let cash = Self::converted_charge(charge_for, &self.owner);
        if self.cooldown_future(true, &future) {
            future.complete(false);
            return;
        }
        if delay <= 0.0 || self.owner.is_authorized("essentials.teleport.timer.bypass") || teleportee.is_authorized("essentials.teleport.timer.bypass") {
            if self.cooldown_future(false, &future) {
                future.complete(false);
                return;
            }
            self.now_async(teleportee, &target, cause, &future);
            if let Some(cash) = &cash {
                cash.charge_future(&self.owner, &future);
                if future.is_completed_exceptionally() {
                    return;
                }
            }
            future.complete(true);
            return;
        }
        self.cancel(false);
        self.warn_user(teleportee, delay);
        self.init_timer((delay * 1000.0) as i64, teleportee, Some(target), cash, cause, false, future);
    }
    fn teleport_other(&self, teleporter: Option<&User>, teleportee: &User, target: Target, charge_for: Option<&Trade>, cause: &'static str, future: EssFuture) {
        let delay = settings().get_teleport_delay();
        let mut cash = charge_for.cloned();
        if let (Some(teleporter), Some(charge)) = (teleporter, charge_for) {
            charge.is_affordable_for_future(teleporter, &future);
            if future.is_completed_exceptionally() {
                return;
            }
            cash = Self::converted_charge(Some(charge), teleporter);
        }
        if self.cooldown_future(true, &future) {
            return;
        }
        if delay <= 0.0 || teleporter.is_none_or(|teleporter| teleporter.is_authorized("essentials.teleport.timer.bypass")) || self.owner.is_authorized("essentials.teleport.timer.bypass") || teleportee.is_authorized("essentials.teleport.timer.bypass") {
            if self.cooldown_future(false, &future) {
                return;
            }
            self.now_async(teleportee, &target, cause, &future);
            if let (Some(teleporter), Some(cash)) = (teleporter, &cash) {
                cash.charge_future(teleporter, &future);
                if future.is_completed_exceptionally() {
                    return;
                }
            }
            future.complete(true);
            return;
        }
        self.cancel(false);
        self.warn_user(teleportee, delay);
        self.init_timer((delay * 1000.0) as i64, teleportee, Some(target), cash, cause, false, future);
    }
    /// `respawn`.
    pub fn respawn(&self, charge_for: Option<&Trade>, cause: &'static str, future: EssFuture) {
        let delay = settings().get_teleport_delay();
        if let Some(charge) = charge_for {
            charge.is_affordable_for_future(&self.owner, &future);
            if future.is_completed_exceptionally() {
                return;
            }
        }
        if self.cooldown_future(true, &future) {
            return;
        }
        if delay <= 0.0 || self.owner.is_authorized("essentials.teleport.timer.bypass") {
            if self.cooldown_future(false, &future) {
                return;
            }
            self.respawn_now(&self.owner.clone(), cause, &future);
            if let Some(charge) = charge_for {
                charge.charge_future(&self.owner, &future);
            }
            future.complete(true);
            return;
        }
        self.cancel(false);
        self.warn_user(&self.owner.clone(), delay);
        self.init_timer((delay * 1000.0) as i64, &self.owner.clone(), None, charge_for.cloned(), cause, true, future);
    }
    pub(crate) fn respawn_now(&self, teleportee: &User, cause: &'static str, future: &EssFuture) {
        let Some(player) = teleportee.player() else {
            future.complete(false);
            return;
        };
        // Java fires a `PlayerRespawnEvent` of its own for the world's spawn, which EssentialsSpawn hears.
        let mut target = player.get_respawn_location().unwrap_or_else(|| player.get_world().get_spawn_location());
        if let Some(hook) = RESPAWN_HOOK.get() {
            target = hook(teleportee, target);
        }
        self.now_async(teleportee, &Target::Location(target), cause, future);
    }
    /// `warp`.
    pub fn warp(&self, other: &User, warp: &str, charge_for: Option<&Trade>, cause: &'static str, future: EssFuture) {
        let location = match ess().warps().get_warp(warp) {
            Ok(location) => location,
            Err(error) => {
                future.complete_exceptionally(error);
                return;
            }
        };
        let (name, teller, owner, place) = (warp.to_owned(), other.clone(), self.owner.clone(), location.clone());
        future.then_accept(move |success| {
            if success {
                let args = crate::args![name.clone(), place.get_world().get_name(), place.get_block_x(), place.get_block_y(), place.get_block_z()];
                teller.send_tl("warpingTo", args);
                if teller != owner {
                    owner.send_tl("warpingTo", args);
                }
            }
        });
        self.teleport_to(other, Target::Location(location), charge_for, cause, future);
    }
    /// `back(chargeFor, future)`.
    pub fn back(&self, charge_for: Option<&Trade>, future: EssFuture) {
        self.back_by(Some(&self.owner.clone()), charge_for, future);
    }
    /// `back(teleporter, chargeFor, future)`.
    pub fn back_by(&self, teleporter: Option<&User>, charge_for: Option<&Trade>, future: EssFuture) {
        self.set_tp_type(TeleportType::Back);
        let Some(location) = self.owner.get_last_location() else {
            future.complete_exceptionally(EssError::tl("noLocationFound", &[]));
            return;
        };
        self.owner.send_tl("backUsageMsg", &teleporting_args(&location));
        self.teleport_other(teleporter, &self.owner.clone(), Target::Location(location), charge_for, "COMMAND", future);
    }
    /// `back(future)`.
    pub fn back_now(&self, future: EssFuture) {
        match self.owner.get_last_location() {
            Some(location) => self.now_async(&self.owner.clone(), &Target::Location(location), "COMMAND", &future),
            None => {
                future.complete_exceptionally(EssError::tl("noLocationFound", &[]));
            }
        }
    }
    /// `cancel`.
    pub(crate) fn cancel(&self, notify_user: bool) {
        let timed = self.state().timed.take();
        if let Some(timed) = timed {
            timed.cancel_timer(notify_user);
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn init_timer(&self, delay: i64, teleportee: &User, target: Option<Target>, charge_for: Option<Trade>, cause: &'static str, respawn: bool, future: EssFuture) {
        let Some(timed) = TimedTeleport::start(self.owner.clone(), delay, future, teleportee, target, charge_for, cause, respawn) else { return };
        self.state().timed = Some(timed);
    }
}

/// `AsyncTimedTeleport`. Java runs the check off-thread and hops back; here the once-a-second check
/// runs on the server thread, in the same order.
pub struct TimedTeleport {
    owner: User,
    teleportee: String,
    started: i64,
    delay: i64,
    future: EssFuture,
    init: (i64, i64, i64),
    target: Option<Target>,
    respawn: bool,
    can_move: bool,
    charge_for: Option<Trade>,
    cause: &'static str,
    task: Mutex<Option<u32>>,
    health: Mutex<f64>,
}

fn moved(location: &Location) -> (i64, i64, i64) {
    (java_round(location.get_x() * MOVE_CONSTANT), java_round(location.get_y() * MOVE_CONSTANT), java_round(location.get_z() * MOVE_CONSTANT))
}

impl TimedTeleport {
    #[allow(clippy::too_many_arguments)]
    fn start(owner: User, delay: i64, future: EssFuture, teleportee: &User, target: Option<Target>, charge_for: Option<Trade>, cause: &'static str, respawn: bool) -> Option<Arc<Self>> {
        let player = teleportee.player()?;
        let can_move = owner.is_authorized("essentials.teleport.timer.move");
        let timed = Arc::new(Self {
            owner,
            teleportee: teleportee.get_uuid(),
            started: now_ms(),
            delay,
            future,
            init: moved(&player.get_location()),
            target,
            respawn,
            can_move,
            charge_for,
            cause,
            task: Mutex::new(None),
            health: Mutex::new(player.get_health()),
        });
        let tick = timed.clone();
        let task = scheduler::run_task_timer(20, 20, move || tick.run());
        *timed.task.lock().unwrap_or_else(|e| e.into_inner()) = Some(task);
        Some(timed)
    }
    fn run(&self) {
        if !self.owner.is_online() {
            self.cancel_timer(false);
            return;
        }
        let Some(user) = ess().get_user_by_uuid(&self.teleportee).filter(User::is_online) else {
            self.cancel_timer(false);
            return;
        };
        let Some(player) = user.player() else {
            self.cancel_timer(false);
            return;
        };
        let health = player.get_health();
        if !self.can_move && (moved(&player.get_location()) != self.init || health < *self.health.lock().unwrap_or_else(|e| e.into_inner())) {
            self.cancel_timer(true);
            return;
        }
        *self.health.lock().unwrap_or_else(|e| e.into_inner()) = health;
        if now_ms() <= self.started + self.delay {
            return;
        }
        let teleport = self.owner.get_async_teleport();
        if let Err(error) = teleport.cooldown(false) {
            self.owner.send_tl("cooldownWithMessage", crate::args![error.get_message()]);
            if self.owner != user {
                user.send_tl("cooldownWithMessage", crate::args![error.get_message()]);
            }
        }
        self.cancel_timer(false);
        teleport.state().timed = None;
        user.send_tl("teleportationCommencing", &[]);
        if let Some(charge) = &self.charge_for
            && let Err(error) = charge.is_affordable_for(&self.owner)
        {
            ess().show_error(&self.owner.get_source(), &error, "\\ teleport");
            return;
        }
        if self.respawn {
            teleport.respawn_now(&user, self.cause, &self.future);
        } else if let Some(target) = &self.target {
            teleport.now_async(&user, target, self.cause, &self.future);
        }
        let (charge, owner) = (self.charge_for.clone(), self.owner.clone());
        self.future.then_accept(move |_| {
            if let Some(charge) = charge
                && let Err(error) = charge.charge(&owner)
            {
                ess().show_error(&owner.get_source(), &error, "\\ teleport");
            }
        });
    }
    /// `cancelTimer`.
    pub fn cancel_timer(&self, notify_user: bool) {
        let Some(task) = self.task.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
        scheduler::cancel_task(task);
        if notify_user {
            self.owner.send_tl("pendingTeleportCancelled", &[]);
            if self.teleportee != self.owner.get_uuid()
                && let Some(user) = ess().get_user_by_uuid(&self.teleportee)
            {
                user.send_tl("pendingTeleportCancelled", &[]);
            }
        }
    }
}

/// The end of `nowAsync`, once the chunk is in.
fn land(teleportee: &User, mut target_location: Location, future: &EssFuture) {
    let Some(player) = teleportee.player() else {
        future.complete(false);
        return;
    };
    let world = target_location.get_world().clone();
    let (x, y, z) = (target_location.get_block_x(), target_location.get_block_y(), target_location.get_block_z());
    if location::is_block_unsafe_for_user(teleportee, &world, x, y, z) {
        if settings().is_teleport_safety_enabled() {
            if settings().is_force_disable_teleport_safety() {
                player.teleport(&target_location);
            } else {
                match location::get_safe_destination_for(teleportee, &target_location) {
                    Ok(safe) => {
                        player.teleport(&safe);
                    }
                    Err(error) => {
                        future.complete_exceptionally(error);
                        return;
                    }
                }
            }
        } else {
            future.complete_exceptionally(EssError::tl("unsafeTeleportDestination", crate::args![world.get_name(), x, y, z]));
            return;
        }
    } else if settings().is_force_disable_teleport_safety() {
        player.teleport(&target_location);
    } else {
        if settings().is_teleport_to_center_location() {
            target_location = location::get_rounded_destination(&target_location);
        }
        player.teleport(&target_location);
    }
    future.complete(true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cooldown_ends_like_gregorian_calendar_adds() {
        assert_eq!(cooldown_end(10_000, 2.5), 12_500);
        assert_eq!(cooldown_end(10_000, -2.5), 7_500);
    }
}
