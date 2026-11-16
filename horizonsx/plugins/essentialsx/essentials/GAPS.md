# What isn't done, and why

Everything here is measured against the real jars on Paper 26.3: all eight modules, the same world seed, the same client.
`plugin-tools/tools/essentials_round.py` builds the bundle and plays two scripts, `essentials_script.py` (every command, bare and used)
and `essentials_script2.py` (a second player, every sign through the editor and a click, Protect with its options on). `essentials_diff.py`
reads the result; `essentials_gate.py` checks a change against a saved run in about a minute. What still differs between those runs is below,
under "What a parity run shows", and nothing else does. Anything not in those scripts is not proven by them: see "Please check in a game".

## How the modules load

`essentials-chat`, `-spawn`, `-antibuild`, `-protect` and `-geoip` are crates of their own beside `essentials`, as asked, but they are
**not separate plugins**. `essentials-bundle` links them with `essentials` into one library, and that's the one to load in place of
`essentials`, under the same name and data folder. Cargo features turn each module off, and `build.rs` adds each module's commands and
permissions to `plugin.yml`.

Why: a plugin here is a library with its own copy of every static, and one can't call into another. In Java a module holds the live
`Essentials` object. Here a module loaded apart would have its own `User`s, teleport timers and caches, and would write the same
`userdata` files over the main plugin's. `Plugin::call_event` crosses plugins but carries a yes or no back, not an answer. Linking is
the honest way to the same objects. `essentials` still builds alone, as before, with its `plugin` feature on.

## SDK

Added to `horizon-bukkit/src/fam_ess.rs`. No engine change.

| Call | Note |
|---|---|
| `World::get_uid` | `uid.dat` in the world's folder, as CraftBukkit keeps it; made if missing. Homes hold it in `world:` |
| `fam_ess::get_player_by_id` | `Bukkit.getPlayer(UUID)` wasn't there |
| `Player::get_max_health` | through the attribute |
| `Player::is_in_water`, `has_passengers`, `eject` | `Entity` has the first; a `Player` isn't an `Entity` in the SDK |
| `Player::get_target_block` | `bukkit~`: the first block a ray stops on. Java's takes the set to see through |
| `Material::is_transparent` | Bukkit's own list |
| `Player::get_max_freeze_ticks`, `set_freeze_ticks`, `set_time_since_rest` | Ask `player_set`, which doesn't know these fields yet: see below |
| `Player::damage(amount, kind)` | `/damage`, a command: `/lightning` |
| `Player::ride(&Entity)` | `/ride`, a command: `/fireball ride` |
| `Player::set_menu_slot` | A slot of the open menu. Nothing in this port calls it now: `/recipe` found it can't reach the table, see "Engine requests" |
| `Sign::get_side_lines`, `set_side_lines` | Either face: `/editsign`. The SDK's `Sign` reads the front alone |
| `TileState::set_spawned_entity` | `CreatureSpawner`: `/spawner` |
| `BanList::add_ban_until` | Through `core_ban_save`, with expiry, source and any reason: `/ban`, `/tempban` and the IP pair. `add_ban` is `/ban`, which has neither |

## Engine requests

None are open that a player can reach. Two things stand in for what the game hasn't got, on purpose:

| What's there | Why |
|---|---|
| `/recipe` is a plugin GUI of type `WORKBENCH`, with the result in slot 0 | The game's own table would hand the items over: `items_menu_set` queues for the next send, and `lan_crafting_closed` gives the grid to the player |
| Stations open a tick after the command, by `inventories::after_turn` | What a player's turn writes to `host.play` is lost when the turn ends |

## Please check in a game

Things that can't be told from here and could hurt if wrong.

- `/recipe` is a plugin GUI with the `minecraft:crafting` menu: look for the grid and the result, and that clicks and drags move nothing.
  Drags are refused too, which Java doesn't do: its `top.clear()` eats what's dropped there, and so would this.
- `/invsee` and `/enderchest` are live, opened a tick after the command. Clicks go by the user's flags, as a live view reports `CHEST`, and
  `InventoryCloseEvent` clears them. If it doesn't fire for a live view, the flags stay up and the next chest follows `/invsee`'s rules.
  Check `essentials.invsee.modify`, `essentials.invsee.preventmodify` and `essentials.enderchest.modify` both ways.
- `/disposal` and the Free sign: what's dropped in `/disposal` should be gone on closing, and the Free sign's stacks should come out.
- `/break` and `/editsign` raise their events: a cancel from another plugin should stop them. `SignChangeEvent` reaches Essentials' own sign
  listener as well, as in Java.
- `@base64` kit lines: `/createkit` with `use-nbt-serialization-in-createkit` on, then the kit. The lines are wrapped at 76 as Java's are, so
  a kit line holds line breaks; `kits.yml` has to keep them. A line that won't decode warns `kitError3` and is skipped, where Java throws.
- `AlternativeCommandsHandler`: load a plugin with a word Essentials has. `/essentials commands` should then list it, `overridden-commands`
  should bring Essentials' back, and a word in `disabled-commands` should go to the other plugin. With no other plugin it runs
  `minecraft:word`, which needs that prefix from the engine, and says `commandDisabled` if the game has no such word.
- `get_commands` and `/help` read `get_known_commands`, which asks the game once for each of its commands. `/powertool` and `/help` call it
  on tab; if that's felt, it wants a cache.
- `/nuke` sets `explosion_power:0f` on each TNT, since `TNTExplodeListener` needs the entity. If the game ignores that field the TNT
  **does real damage**.
- `/antioch` is real TNT in Java too.
- `kick_player` is `/kick name reason`: a reason with a line break (`|` or `\n`, as `/ban` and `/kick` take) may not parse.
- Nested data in `Entity::merge`, used by all of `MobData`: `equipment`, `drop_chances`, `VillagerData`, `anger_end_time`,
  `attributes`, and the names `variant`, `Variant`, `Type`, `type`. These are the saved forms of 26.x as far as known, unproven.
- `core_ban_save` reads the file, adds a row and reloads. Check a `/tempban` expires, and that a name with no profile gets a row.
- `/tpr` forces a chunk with a ticket and polls for up to 10 seconds per attempt. With `find-attempts: 10` a slow world may take long.
- `set_storm` is for every level: `/weather` in one world changes all of them.

## SDK behaviour worth knowing

- `World::get_block_at` answers air for a level that isn't active, and a chunk not in. So a teleport to another dimension or an unloaded
  chunk can't be judged: `AsyncTeleport.now_async` checks `is_chunk_loaded` and lands as asked, with no safety check.
- `Player.teleport` is a command: it lands at the end of the tick. Anything after it in the same tick sees the old place.
- `Player.set_item`, `give` and friends are commands too. `Inventories.add_item` keeps its own record so a split stack doesn't lose count,
  but two separate calls in a tick read stale slots.
- `permissions`: `children:` in `plugin.yml` aren't followed by the SDK for grants. `essentials.exempt` and `essentials.seen.extra` won't
  give their children.
- `FoodLevelChangeEvent` is a tick late, as the SDK says; god mode puts the food back.
- `/time`, `/ptime` compute the new time from what they set, as `world.set_time` lands late, rather than reading it back.

## Left out

| Class | Why | Stands in the way of |
|---|---|---|
| `EssentialsUpgrade` (1123 lines) | Migrates files from older EssentialsX. A server on 2.22.0 needs none of it; one on an older version must run 2.22.0 once first | a move from an old version |
| `LegacyItemDb`, `items.csv` | Java doesn't use them from 1.13 on either | |
| `EconomyLayers`, Vault | No Vault plugin here: `Economy` is the store itself, and it provides `net.milkbowl.vault.economy.Economy` as a named service any plugin can call | |
| `PermissionsHandler` providers, LuckPerms contexts | No such plugin here | groups, prefixes, suffixes: `get_group` is `default`, prefix and suffix are empty |
| Essentials' API events (`UserBalanceUpdateEvent`, `HomeModifyEvent`, `SignInteractEvent` and the rest) | For other Java plugins, and none can hear them. The eight the modules need are `essentials::hooks` | plugins built on EssentialsX's API |
| `MetricsWrapper`, `EssentialsLogger`, `ExecuteTimer`, `ManagedFile` | `items.json` is written if missing, but its version isn't checked against the shipped one | |
| `EssentialsPluginListener` | `AlternativeCommandsHandler` checks the plugin list at each look instead | |
| Protect's emergency mode | For Essentials failing to load, which in one library is this code not running either | |

## What a parity run shows

These are all that's left between Paper and this, in a run of both scripts:

- `Server version` and `Brand version`: this isn't Paper, and says so.
- `/help`'s first page of commands: Java's order there is `HashMultimap`'s of objects hashed by identity, so it changes with every start of
  Paper itself. The lists compare as sets; a first page of a random order has nothing to compare.
- A line from another thread (`/skull`, the update check) lands a line or two sooner or later, as it does between two runs on Paper.
- Where a player is a moment after a teleport, at the scripts' fast pace: each client gets its chunks at its own speed. Given time they agree.
- Numbers of mobs and chunks, and clocks.

## Smaller differences

- Translations load at once. Java returns English until the bundle has loaded, for the very first line.
- `Regex`, not `java.util.regex`: a command cooldown pattern or nick blacklist with look-around or a back-reference doesn't compile. A
  cooldown like that is kept but never matches; a blacklist entry is dropped with Java's warning.
- `homes` in a user's file keep file order; a user who has just set their first homes this session sees insertion order, where Java's fresh
  `HashMap` has its hash order until the next load. `/kit` and `/jails` follow Java's `HashMap` order.
- `NumberFormat` grouping marks come from a small table by language, and `currency-symbol-format-locale` likewise.
- `DateFormat.getDateTimeInstance` in `trade.log`, `formatDate`, `/balancetop`'s header, and `{TIME}` and `{DATE}` are English patterns.
- `enchantment_max_level` is a table: the SDK has no `getMaxLevel`. Datapack enchantments count as 1. `can_enchant_item` is a table of
  the game's `supported_items` tags, by item name.
- `{WORLDDATE}` gives `N/A`.
- `FlatItemDb.get`, for a potion, sets the base potion by `long_`/`strong_` id. `upgraded` and `extended` for a potion that has neither aren't checked.
- `command-cooldowns` entries that fail to compile are skipped with a warning.
- Block command senders: `CommandSender` has no such case, so a command block's command shows as the console's.
- `EssFuture.then_accept` of a timed teleport runs on the server thread; Java's may run on its timer thread. Same order.
- `setTotalExperience` hands the whole sum to one `give_exp`, since each level of Java's loop reads back what a queued command hasn't done yet.
- `/seen` and `/essentials` run on a thread of their own only what reads files, and answer on the server's, where Java sends from its thread.
- `/kill` and `/suicide` raise no damage event; `/kill` checks god mode in its place.
- `/invsee` shows the 41 slots in a 45-slot window with the last four hidden, titled `Player`. `/enderchest` shows `container.enderchest`.
- `/invsee`, `/enderchest` and the station commands open a tick after the command, not during it.
- `/recipe`'s window is titled `Crafting` in English, whatever the client's language, as a plugin GUI's title is a plain string.
- A completer of another plugin that answers nothing leaves Essentials' suggestions standing, where Java would show none: this can't tell it
  from a command with no completer.
- `/recipe` for tagged ingredients shows the tag's first item, as `RecipeChoice.getItemStack` does.
- `/remove` lists its entity classes by hand, from the game's types. A datapack or new entity won't be in them.
- `/time` sorts worlds by name, as Java's `TreeSet` does. `/worlds` created through `horizon_bukkit::worlds` are all in `get_worlds`.

## EssentialsXMPP

`essentials-xmpp`, linked into the bundle like the other modules, reads the same `config.yml` and `users.yml` in `plugins/EssentialsXMPP`.
Smack is replaced by a small client in `src/client.rs` on `rustls`. Its tests run it against a fake server on this machine, and it has
never talked to a real XMPP server. What differs from Java, on purpose:

| Java | Here | Why |
|---|---|---|
| Logs in on the main thread | On a thread of its own | A slow server would stop the game |
| `sendMessage` reconnects on the main thread | Queues up to 1,024 and reconnects on its own thread | The same |
| SASL: DIGEST-MD5, PLAIN and others | SCRAM-SHA-1, then PLAIN | DIGEST-MD5 is gone from modern servers. The default `sasl-enabled: false` is still the old `jabber:iq:auth` login, which most modern servers refuse; a clear error says to turn SASL on |
| Trusts the JVM's root certificates | Trusts the Mozilla roots in `webpki-roots` | A server with a private certificate won't connect: there's no setting for it, as Java has none |
| No warning without TLS | The login still goes ahead, with a warning | The password crosses the network unencrypted, as it did in Java |
| `[` + an object's hash + `>` in `@name` messages | The sender's name | Java prints `User`'s default `toString`, which is a bug |
| Answers no pings | Answers `urn:xmpp:ping`, refuses other requests | RFC 6120 wants an answer, and servers can drop a client that gives none |
| `debug: true` opens Smack's debug window | Ignored | There's no such window |
| bStats `config-valid` | Left out | No metrics here |
| A log record's class and method in forwarded logs | `EssentialsXMPP` | The Rust logger has no caller |

Log forwarding covers this module's own log lines, as Java's handler on its own logger did.

## EssentialsDiscord and EssentialsDiscordLink

`essentials-discord` and `essentials-discordlink` are in the bundle like the other modules, with `on_enable` and `on_disable`, reading the same
`config.yml`s and `accounts.json` in `plugins/EssentialsDiscord` and `plugins/EssentialsDiscordLink`. JDA and discord-webhooks are replaced by a
small blocking client in `essentials-discord/src/client`: `tungstenite` and `rustls` for the gateway, `ureq` for REST, on threads of its own.
`essentials_discord::service()` is `DiscordService`, which DiscordLink calls directly. **Nothing here has ever talked to the real Discord.** The
tests run it against a fake gateway and REST server on 127.0.0.1.

**Never tried against the real service:** login with a real token, IDENTIFY and its intents, Discord's rate limit headers as they really come,
slash commands appearing in a client, real webhooks, real interaction deadlines, REST over TLS (`ureq`'s own; the gateway did run over TLS, against a
local certificate), and every message's look in a Discord client. Nothing in the game has run either: `listeners.rs`, `commands.rs`,
`interaction_commands.rs`, `recipient.rs`, all of DiscordLink's `listeners.rs`, `manager.rs` and `rolesync.rs`'s game half compile and have never run.
The intents are JDA's `DEFAULT` as I remember it: JDA isn't on disk to check against, and the bits that matter are `GUILDS`, `GUILD_MESSAGES`,
`GUILD_EXPRESSIONS` and `MESSAGE_CONTENT`. A bot needs the Message Content intent turned on in the developer portal or Discord closes with 4014, which stops
for good.

The token, webhook tokens and interaction tokens are `Secret`s, which don't print. REST errors name the route's template and never its URL, and `host::log`
takes any known token out of every line. A test logs in with a refused token and looks for it in the log.

| Java | Here | Why |
|---|---|---|
| `awaitReady` has no limit | `on_enable` still blocks for Discord, but gives up after 60 seconds with `discordErrorLogin` | Java can hang a server's start forever |
| A failed `getMemberById` counts as someone who left, so role sync unlinks them | `try_member_by_id` keeps the two apart: only a real 404 unlinks | A network hiccup shouldn't wipe a player's link. `get_member_by_id` itself stays as Java's, for mail |
| A 429 in `WebhookDispatcher` fails the message, and a failure ends the console relay | The message stays at the head of the queue, and the wait is at least Discord's `retry_after` | The library behind the Java retries by itself, so that path barely ran there |
| `sendMessage(SERVER_STOP)` and then `dispatchDiscordMessage` | Only the second | The first goes to a listener of a plugin being disabled, which Bukkit skips |
| `DiscordCommandSender` collects output on a timer | The SDK's `create_command_sender` answers at once | Nothing to wait for |
| `getDiscriminator` of a migrated user | `0000`, as I remember JDA doing | Not checked against JDA |
| `{starttimeseconds}` is from the JVM's start | From when Essentials started | There's no JVM |
| `Level.toLevel`, `log4j` logger names | Levels are the same. `[LoggerName]` is left out of console lines, as the log file has the thread and not the logger | See below |
| emoji-java's alias table | The `emojis` crate's, from gemoji | An alias can differ. An emoji with no alias stays as it is |
| A MESSAGE_CREATE is handled on JDA's thread | Filtered and formatted on the gateway thread, and shown on the main thread | Game calls stay on the main thread, so a line can reach players a tick later |
| `/link` and the like may run on any thread | The role sync's game half is on the main thread | The same |
| With Discord down, `onEnable` of DiscordLink dies on a null `api` | It logs a line and stays off | |
| `permanent-mute` and such | The same defaults as the Java, including its mismatch with `config.yml` | Copied as it is |

| Left out | Why |
|---|---|
| PlaceholderAPI | Optional in the Java, and no such plugin here. Its placeholders stay as text |
| `Unsafe.getJDAInstance` | There's no JDA. `service().discord()` is the nearest |
| `DiscordRelayEvent`, `DiscordChatMessageEvent`, `DiscordLinkStatusChangeEvent` as events | No other library can hear them. Link's own listener is called directly |
| `essentials:linked` context, metrics | LuckPerms and bStats aren't here |
| The JDK-8274349 mitigation, version-mismatch warning | No JVM, and it's one library |
| `DiscordUtil.cleanWebhooks` | The Java never calls it |

**Role sync goes one way.** `PermissionsHandler` here has no `addToGroup` or `removeFromGroup`, and everyone is in `default`. So a group gives a role, but a
role can't give a group: `role-sync.roles` rows are checked and then do nothing, and `remove-groups` does nothing. On Java, this is true of a server
with no permissions plugin too.

**Engine request: a tap on the server's log.** `ConsoleInjector` hangs an appender on Log4j. The SDK has no tap, so `console.rs` reads the game's own
`logs/latest.log` as it grows, a few times a second. That gets every line, its level and its text, but not the logger's name, so `[LoggerName]` is missing, and
a line that isn't in the pattern counts as `INFO`. A real tap would be a hook with level, logger and message. If a server writes no `latest.log`, the relay is
silent: `horizon_client::logging` won't open one if another Horizon holds the lock.

**Please check in a game**

- `FeedbackCommandSender::dispatch` is `core_run_captured`, which captures chat. Whether a *plugin's* command, Essentials' own included, sends its output there is
  unknown: `/execute` and the console command relay may show nothing for those. A command it can't run goes to the console with no reply, as Java's vanilla fallback does.
- `Component::plain` of an advancement's title: if it gives a translation key rather than English, `{advancement}` will read like a key.
- `showDeathMessages` and `announceAdvancements` are read by those names through `get_game_rule_value`.
- `hasPlayedBefore` is any time played, as in EssentialsSpawn. So `first-join` depends on the stats file.
- `/link`'s policy `kick` on `AsyncPlayerPreLoginEvent`, and `freeze`, have never run.
