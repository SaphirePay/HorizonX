# Adding a command

Each of the original's `commands/CommandX.java` is one file here, `src/commands/x.rs`. Nothing else changes:
`build.rs` lists every file in `src/commands`, and keeps `plugin.yml` to the commands that have one, so the word
reaches the game the moment the file exists, and never before. Don't edit `plugin.yml` (it's generated from
`plugin-original.yml`), `commands/mod.rs` or `commands/base.rs`.

A file named after a Rust keyword takes `cmd_` in front: `Commandbreak` is `src/commands/cmd_break.rs`. The name in
`COMMAND` stays `"break"`.

## The file

```rust
//! `Commandhat`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("hat") };

fn run(user: &User, label: &str, args: &[String]) -> EssResult {
    Ok(())
}
```

`prelude.rs` has what most files use, all of `commands/base.rs` among it: name only what isn't there.

`Command` names what Java overrides. What's left out behaves as `EssentialsCommand`'s default does:

| Field | Java | Left out |
|---|---|---|
| `run_user` | `run(Server, User, String, String[])` | runs `run_source` with `user.get_source()` |
| `run_source` | `run(Server, CommandSource, String, String[])` | `onlyPlayers`, for a console |
| `tab_user` | `getTabCompleteOptions(Server, User, ...)` | `tab_source` |
| `tab_source` | `getTabCompleteOptions(Server, CommandSource, ...)` | the names of players |

The dispatch around these is `onCommandEssentials`, all of it: the disabled list, `essentials.<name>` (and `essentials.msg`
for `r`), `essentials.jail.allow.<name>`, the mail reminder, the command's cost checked before and charged after, and
what to show for each error. So a command only has what's in its Java `run`. Don't check `essentials.<name>` again.

A tab function gives every option, and the dispatch filters by what's typed, as `tabComplete` does. `None` is Java's `null`.

## Line by line

Keep the checks in Java's order, and each failure on its own message. `throw` is `return Err(...)`.
`if (x) throw ...;` is `fail_if!(x, error);`, and a value that must be there is `value.ok_or_else(EssError::not_enough)?`.

| Java | Here |
|---|---|
| `throw new NotEnoughArgumentsException()` | `return Err(EssError::not_enough())` |
| `throw new TranslatableException("key", a, b)` | `return Err(EssError::tl("key", args![a, b]))` |
| `throw new NoChargeException()` | `return Err(EssError::NoCharge)` |
| `throw new QuietAbortException()` | `return Err(EssError::QuietAbort)` |
| `throw new PlayerNotFoundException()` | `return Err(EssError::player_not_found())` |
| `throw new PlayerExemptException("key")` | `return Err(EssError::player_exempt("key", &[]))` |
| `throw new Exception(text)` | `return Err(EssError::Message(text))` |
| `ess.getSettings().isX()` | `settings().is_x()` (from `crate::ess`) |
| `ess.getWarps()`, `getKits`, `getJails`, `getWorth`, `getItemDb`, `getUsers`, `getBackup`, `getBalanceTop` | `ess().warps()` and so on; `item_db()` |
| `user.isAuthorized(node)` | `user.is_authorized(node)` |
| `user.sendTl("key", a, b)` | `user.send_tl("key", args![a, b])` |
| `AdventureUtil.parsed(text)` | `parsed(text)` from `crate::i18n`, inside `args!` |
| `NumberUtil.displayCurrency(x, ess)` | `display_currency(&x)`, always in `parsed(...)` |
| `user.getBase()` | `user.player()`: `None` for someone offline, the `OfflinePlayerStub` case |
| `user.getBase().isOnline()` | `user.is_online()` |
| `user.getBase() instanceof OfflinePlayerStub` | `user.is_offline_stub()` |
| `user.getLocation()`, `getWorld()` | the same, `Option` for someone offline |
| `sender.isPlayer()`, `getPlayer()`, `getUser()` | the same; `get_player`, `get_user` are `Option` |
| `getPlayer(server, sender, args, pos)` | `get_player(sender, args, pos)` |
| `getPlayer(server, sender, args, pos, true)` | `get_player_or_offline(sender, args, pos, true)` |
| `getPlayer(server, args, pos, hidden, offline)` | `get_player_raw(None, args, pos, hidden, offline)` |
| `getPlayer(server, user, args, pos)` | `get_player_raw(Some(user), args, pos, user.can_interact_vanished(), false)` |
| `getPlayer(server, term, hidden, offline)` | `get_player_term_raw(term, hidden, offline)` |
| `getPlayer(server, sender, term)` | `get_player_by_term(sender, term)` |
| `getFinalArg(args, n)` | `get_final_arg(args, n)` |
| `name.split(":")` | `java_split(name, ':')`, which drops trailing empties as Java does |
| `this.getName()` in `new Trade(...)` | the name as text |
| `new Trade(BigDecimal, ess)` etc | `Trade::of_money`, `of_item`, `of_exp`, `of_command`, `of_command_with_fallback` |
| `getNewExceptionFuture(sender, label)` | `get_new_exception_future(&sender, label)` |
| `TeleportCause.COMMAND` | `"COMMAND"` |
| `user.getAsyncTeleport().teleport(loc, charge, cause, future)` | the same, `Some(&charge)` or `None`, `&loc` |
| `BigDecimal` | `crate::big_decimal::BigDecimal`; `==` and `<` compare as `compareTo`, `equals_exact` is `equals` |
| `Math.round` | `java_round` from `utils::location`: Rust's rounds differently |
| a `HashMap` printed as it comes | `java_hash_map_order`, or chat comes out in another order |

`"%s"` numbers in a message are `MessageFormat`'s: pass an `i32`, `usize` or `f64`, and it groups them. A number passed as
text won't be grouped, so keep Java's type. Text arguments are escaped and their section signs turned to tags, as Java does.

Commands that fire Essentials' API events (`HomeModifyEvent`, `FlyStatusChangeEvent` and others): leave the event out. There's
nothing to listen to it. Note it in the doc line at the top, as `fly.rs` does.

`EssentialsToggleCommand`: `toggle_command!("name", "others.perm")`, with a `toggle_player`, as in `fly.rs`. A label that decides the state, as in `paytoggle.rs`, writes its `COMMAND` by hand.
`EssentialsLoopCommand`: `loop_online_players`, with `update_player` as a closure, as in `heal.rs`.

A call that needs the engine goes under "Engine requests" in GAPS.md, and the command goes as far as it can. Where nothing can happen it says Java's own `unsupportedFeature`, as `ice.rs` does: it never answers as if it worked.

A listener copies its `@EventHandler`: `plugin.register_event_with(LOW, true, ...)` for `priority = LOW, ignoreCancelled = true`. Two Java handlers at different priorities are two registrations.

Check with `cargo +1.99.1 check --offline` here, with your own `CARGO_TARGET_DIR`. A pure function
gets a `#[test]`.

## A worked example: `Commandfly`

The Java, in full (`togglePlayer` is the only thing it has of its own):

```java
public class Commandfly extends EssentialsToggleCommand {
    public Commandfly() { super("fly", "essentials.fly.others"); }
    protected void run(Server server, CommandSource sender, String commandLabel, String[] args) throws Exception {
        toggleOtherPlayers(server, sender, args);
    }
    protected void run(Server server, User user, String commandLabel, String[] args) throws Exception {
        handleToggleWithArgs(server, user, args);
    }
    protected void togglePlayer(CommandSource sender, User user, Boolean enabled) {
        if (enabled == null) { enabled = !user.getBase().getAllowFlight(); }
        FlyStatusChangeEvent event = new FlyStatusChangeEvent(...);
        ess.getServer().getPluginManager().callEvent(event);
        if (!event.isCancelled()) {
            user.getBase().setFallDistance(0f);
            user.getBase().setAllowFlight(enabled);
            user.setFlyModeEnabled(enabled);
            if (!user.getBase().getAllowFlight()) { user.getBase().setFlying(false); }
            user.sendTl("flyMode", CommonPlaceholders.enableDisable(user.getSource(), enabled), user.getDisplayName());
            if (!sender.isPlayer() || !sender.getPlayer().equals(user.getBase())) {
                sender.sendTl("flyMode", CommonPlaceholders.enableDisable(user.getSource(), enabled), user.getDisplayName());
            }
        }
    }
}
```

`src/commands/fly.rs`:

```rust
//! `Commandfly`, an `EssentialsToggleCommand`.
//!
//! `FlyStatusChangeEvent` isn't fired: nothing could listen.
use crate::prelude::*;

toggle_command!("fly", "essentials.fly.others");

fn toggle_player(sender: &CommandSource, user: &User, enabled: Option<bool>) -> EssResult {
    let Some(player) = user.player() else {
        return Ok(());
    };
    let enabled = enabled.unwrap_or_else(|| !player.get_allow_flight());
    player.set_fall_distance(0.0);
    player.set_allow_flight(enabled);
    user.set_fly_mode_enabled(enabled);
    if !enabled {
        player.set_flying(false);
    }
    let args = crate::args![enable_disable(&user.get_source(), enabled), user.get_display_name()];
    user.send_tl("flyMode", args);
    if !is_self(sender, user) {
        sender.send_tl("flyMode", args);
    }
    Ok(())
}
```

What was left out, and where it's said: the event isn't fired. `toggle_command!` writes the `COMMAND` of a toggle, which is all
that's the same in each one.
Notice `setAllowFlight` reads back at once in Java; here `enabled` is used, as it was just set. A setter on `Player` that goes
through a command lands at the end of the tick, so don't read one back.

The other six are in the same folder: `home.rs` (futures, teleport, a tab list), `sethome.rs`, `delhome.rs` (a console path, `*`),
`balance.rs` (both run functions), `warp.rs` (paging, charges), `heal.rs` (a loop command).

# Every entry of commands/

<!-- table -->

| Original | State | Here |
|---|---|---|
| Commandafk.java | **done** | src/commands/afk.rs |
| Commandantioch.java | **done** | src/commands/antioch.rs |
| Commandanvil.java | **done** | src/commands/anvil.rs |
| Commandback.java | **done** | src/commands/back.rs |
| Commandbackup.java | **done** | src/commands/backup.rs |
| Commandbalance.java | **done** | src/commands/balance.rs |
| Commandbalancetop.java | **done** | src/commands/balancetop.rs |
| Commandban.java | **done** | src/commands/ban.rs |
| Commandbanip.java | **done** | src/commands/banip.rs |
| Commandbeezooka.java | **done** | src/commands/beezooka.rs |
| Commandbigtree.java | **done** | src/commands/bigtree.rs |
| Commandbook.java | **done** | src/commands/book.rs |
| Commandbottom.java | **done** | src/commands/bottom.rs |
| Commandbreak.java | **done** | src/commands/cmd_break.rs |
| Commandbroadcast.java | **done** | src/commands/broadcast.rs |
| Commandbroadcastworld.java | **done** | src/commands/broadcastworld.rs |
| Commandburn.java | **done** | src/commands/burn.rs |
| Commandcartographytable.java | **done** | src/commands/cartographytable.rs |
| Commandclearinventory.java | **done** | src/commands/clearinventory.rs |
| Commandclearinventoryconfirmtoggle.java | **done** | src/commands/clearinventoryconfirmtoggle.rs |
| Commandcompass.java | **done** | src/commands/compass.rs |
| Commandcondense.java | **done** | src/commands/condense.rs |
| Commandcreatekit.java | **in part** | src/commands/createkit.rs: no paste upload |
| Commandcustomtext.java | **done** | src/commands/customtext.rs |
| Commanddelhome.java | **done** | src/commands/delhome.rs |
| Commanddeljail.java | **done** | src/commands/deljail.rs |
| Commanddelkit.java | **done** | src/commands/delkit.rs |
| Commanddelwarp.java | **done** | src/commands/delwarp.rs |
| Commanddepth.java | **done** | src/commands/depth.rs |
| Commanddisposal.java | **done** | src/commands/disposal.rs |
| Commandeco.java | **done** | src/commands/eco.rs |
| Commandeditsign.java | **done** | src/commands/editsign.rs |
| Commandenchant.java | **done** | src/commands/enchant.rs |
| Commandenderchest.java | **done** | src/commands/enderchest.rs |
| Commandessentials.java | **in part** | src/commands/essentials.rs: `dump` can't upload, `version` can't check |
| Commandexp.java | **done** | src/commands/exp.rs |
| Commandext.java | **done** | src/commands/ext.rs |
| Commandfeed.java | **done** | src/commands/feed.rs |
| Commandfireball.java | **done** | src/commands/fireball.rs |
| Commandfirework.java | **done** | src/commands/firework.rs |
| Commandfly.java | **done** | src/commands/fly.rs |
| Commandgamemode.java | **done** | src/commands/gamemode.rs |
| Commandgc.java | **done** | src/commands/gc.rs |
| Commandgetpos.java | **done** | src/commands/getpos.rs |
| Commandgive.java | **done** | src/commands/give.rs |
| Commandgod.java | **done** | src/commands/god.rs |
| Commandgrindstone.java | **done** | src/commands/grindstone.rs |
| Commandhat.java | **done** | src/commands/hat.rs |
| Commandheal.java | **done** | src/commands/heal.rs |
| Commandhelp.java | **done** | src/commands/help.rs |
| Commandhelpop.java | **done** | src/commands/helpop.rs |
| Commandhome.java | **done** | src/commands/home.rs |
| Commandice.java | **in part** | src/commands/ice.rs: `unsupportedFeature` until the engine call |
| Commandignore.java | **done** | src/commands/ignore.rs |
| Commandinfo.java | **done** | src/commands/info.rs |
| Commandinvsee.java | **done** | src/commands/invsee.rs |
| Commanditem.java | **done** | src/commands/item.rs |
| Commanditemdb.java | **done** | src/commands/itemdb.rs |
| Commanditemlore.java | **done** | src/commands/itemlore.rs |
| Commanditemname.java | **done** | src/commands/itemname.rs |
| Commandjailedplayers.java | **done** | src/commands/jailedplayers.rs |
| Commandjails.java | **done** | src/commands/jails.rs |
| Commandjump.java | **done** | src/commands/jump.rs |
| Commandkick.java | **done** | src/commands/kick.rs |
| Commandkickall.java | **done** | src/commands/kickall.rs |
| Commandkill.java | **done** | src/commands/kill.rs |
| Commandkit.java | **done** | src/commands/kit.rs |
| Commandkitreset.java | **done** | src/commands/kitreset.rs |
| Commandkittycannon.java | **done** | src/commands/kittycannon.rs |
| Commandlightning.java | **done** | src/commands/lightning.rs |
| Commandlist.java | **done** | src/commands/list.rs |
| Commandloom.java | **done** | src/commands/loom.rs |
| Commandmail.java | **done** | src/commands/mail.rs |
| Commandme.java | **done** | src/commands/me.rs |
| Commandmore.java | **done** | src/commands/more.rs |
| Commandmotd.java | **done** | src/commands/motd.rs |
| Commandmsg.java | **done** | src/commands/msg.rs |
| Commandmsgtoggle.java | **done** | src/commands/msgtoggle.rs |
| Commandmute.java | **done** | src/commands/mute.rs |
| Commandnear.java | **done** | src/commands/near.rs |
| Commandnick.java | **done** | src/commands/nick.rs |
| Commandnuke.java | **done** | src/commands/nuke.rs |
| Commandpay.java | **done** | src/commands/pay.rs |
| Commandpayconfirmtoggle.java | **done** | src/commands/payconfirmtoggle.rs |
| Commandpaytoggle.java | **done** | src/commands/paytoggle.rs |
| Commandping.java | **done** | src/commands/ping.rs |
| Commandplaytime.java | **done** | src/commands/playtime.rs |
| Commandpotion.java | **done** | src/commands/potion.rs |
| Commandpowertool.java | **done** | src/commands/powertool.rs |
| Commandpowertoollist.java | **done** | src/commands/powertoollist.rs |
| Commandpowertooltoggle.java | **done** | src/commands/powertooltoggle.rs |
| Commandptime.java | **done** | src/commands/ptime.rs |
| Commandpweather.java | **done** | src/commands/pweather.rs |
| Commandr.java | **done** | src/commands/r.rs |
| Commandrealname.java | **done** | src/commands/realname.rs |
| Commandrecipe.java | **done** | src/commands/recipe.rs |
| Commandremove.java | **done** | src/commands/remove.rs |
| Commandrenamehome.java | **done** | src/commands/renamehome.rs |
| Commandrepair.java | **done** | src/commands/repair.rs |
| Commandrest.java | **in part** | src/commands/rest.rs: `unsupportedFeature` until the engine call |
| Commandrtoggle.java | **done** | src/commands/rtoggle.rs |
| Commandrules.java | **done** | src/commands/rules.rs |
| Commandseen.java | **done** | src/commands/seen.rs |
| Commandsell.java | **done** | src/commands/sell.rs |
| Commandsethome.java | **done** | src/commands/sethome.rs |
| Commandsetjail.java | **done** | src/commands/setjail.rs |
| Commandsettpr.java | **done** | src/commands/settpr.rs |
| Commandsetwarp.java | **done** | src/commands/setwarp.rs |
| Commandsetworth.java | **done** | src/commands/setworth.rs |
| Commandshowkit.java | **done** | src/commands/showkit.rs |
| Commandskull.java | **done** | src/commands/skull.rs |
| Commandsmithingtable.java | **done** | src/commands/smithingtable.rs |
| Commandsocialspy.java | **done** | src/commands/socialspy.rs |
| Commandspawner.java | **done** | src/commands/spawner.rs |
| Commandspawnmob.java | **done** | src/commands/spawnmob.rs |
| Commandspeed.java | **done** | src/commands/speed.rs |
| Commandstonecutter.java | **done** | src/commands/stonecutter.rs |
| Commandsudo.java | **done** | src/commands/sudo.rs |
| Commandsuicide.java | **done** | src/commands/suicide.rs |
| Commandtempban.java | **done** | src/commands/tempban.rs |
| Commandtempbanip.java | **done** | src/commands/tempbanip.rs |
| Commandthunder.java | **done** | src/commands/thunder.rs |
| Commandtime.java | **done** | src/commands/time.rs |
| Commandtogglejail.java | **done** | src/commands/togglejail.rs |
| Commandtop.java | **done** | src/commands/top.rs |
| Commandtp.java | **done** | src/commands/tp.rs |
| Commandtpa.java | **done** | src/commands/tpa.rs |
| Commandtpaall.java | **done** | src/commands/tpaall.rs |
| Commandtpacancel.java | **done** | src/commands/tpacancel.rs |
| Commandtpaccept.java | **done** | src/commands/tpaccept.rs |
| Commandtpahere.java | **done** | src/commands/tpahere.rs |
| Commandtpall.java | **done** | src/commands/tpall.rs |
| Commandtpauto.java | **done** | src/commands/tpauto.rs |
| Commandtpdeny.java | **done** | src/commands/tpdeny.rs |
| Commandtphere.java | **done** | src/commands/tphere.rs |
| Commandtpo.java | **done** | src/commands/tpo.rs |
| Commandtpoffline.java | **done** | src/commands/tpoffline.rs |
| Commandtpohere.java | **done** | src/commands/tpohere.rs |
| Commandtppos.java | **done** | src/commands/tppos.rs |
| Commandtpr.java | **done** | src/commands/tpr.rs |
| Commandtptoggle.java | **done** | src/commands/tptoggle.rs |
| Commandtree.java | **done** | src/commands/tree.rs |
| Commandunban.java | **done** | src/commands/unban.rs |
| Commandunbanip.java | **done** | src/commands/unbanip.rs |
| Commandunlimited.java | **done** | src/commands/unlimited.rs |
| Commandvanish.java | **done** | src/commands/vanish.rs |
| Commandwarp.java | **done** | src/commands/warp.rs |
| Commandwarpinfo.java | **done** | src/commands/warpinfo.rs |
| Commandweather.java | **done** | src/commands/weather.rs |
| Commandwhois.java | **done** | src/commands/whois.rs |
| Commandworkbench.java | **done** | src/commands/workbench.rs |
| Commandworld.java | **done** | src/commands/world.rs |
| Commandworth.java | **done** | src/commands/worth.rs |
| EssentialsCommand.java | foundation | base.rs: `Command`, `get_player*`, `get_players`, `get_final_arg`, dispatch |
| EssentialsLoopCommand.java | foundation | base.rs: `loop_online_players`, `loop_offline_players`, `get_players_with_wildcards` |
| EssentialsToggleCommand.java | foundation | base.rs: `handle_toggle_with_args`, `toggle_other_players`, `toggle_tab_*` |
| EssentialsTreeCommand.java | foundation | essentials.rs: the two matches at its end |
| EssentialsTreeNode.java | foundation | essentials.rs: an arm of those matches |
| IEssentialsCommand.java | foundation | base.rs: the `Command` struct |
| InvalidModifierException.java | foundation | error.rs: `EssError::invalid_modifier()` |
| NoChargeException.java | foundation | error.rs: `EssError::NoCharge` |
| NotEnoughArgumentsException.java | foundation | error.rs: `EssError::not_enough()` |
| PlayerExemptException.java | foundation | error.rs: `EssError::player_exempt` |
| PlayerNotFoundException.java | foundation | error.rs: `EssError::player_not_found()` |
| QuietAbortException.java | foundation | error.rs: `EssError::QuietAbort` |
| WarpNotFoundException.java | foundation | error.rs: `EssError::warp_not_found()` |
| essentials | foundation | subfolder: essentials.rs |
