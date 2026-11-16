# AdvancedBan 2.3.0: rewrite brief

Main class `me.leoko.advancedban.bukkit.BukkitMain`. 50 own classes, 176 KB of Java (java/).

## Commands

- `/AdvancedBan`: Information about the Plugin
- `/ban`: Ban a user permanently
- `/tempban`: Ban a user temporarily
- `/ipban` aliases ['banip', 'ban-ip']: Ban a user by IP
- `/tempipban` aliases ['tipban']: Ban a user temporarily by IP
- `/kick`: Kick a user
- `/warn`: Warn a user permanently
- `/note`: Adds a note to a users file
- `/tempwarn`: Warn a user temporarily
- `/mute`: Mute a user permanently
- `/tempmute`: Mute a user temporarily
- `/banlist`: See all punishments
- `/history`: See a user''s history
- `/warns`: See your or a user''s warns
- `/notes`: See your or a user''s notes
- `/check`: Get all information about a user
- `/systemPrefs`: Displays System-Preferences for setting up the plugin
- `/unwarn`: Deletes a warn
- `/unnote`: Deletes a note
- `/unpunish`: Deletes a punishment by ID
- `/unban`: Unban a user
- `/unmute`: Unmute a user
- `/change-reason`: Change the reason of an existing punishment

## Sub-commands found in the source

- `ban` (me/leoko/advancedban/utils/Command.java:201)
- `clear` (me/leoko/advancedban/utils/Command.java:127)
- `mute` (me/leoko/advancedban/utils/Command.java:201)

## Permissions


## Events

- ChatEvent -> onChat (me/leoko/advancedban/bungee/listener/ChatListenerBungee.java)
- PlayerDisconnectEvent -> onDisconnect (me/leoko/advancedban/bungee/listener/ConnectionListenerBungee.java) **no SDK event of this name**
- PlayerJoinEvent -> onJoin (me/leoko/advancedban/bukkit/listener/ConnectionListener.java)
- PlayerQuitEvent -> onDisconnect (me/leoko/advancedban/bukkit/listener/ConnectionListener.java)
- PluginMessageEvent -> onPluginMessageEvent (me/leoko/advancedban/bungee/listener/InternalListener.java) **no SDK event of this name**
- PostLoginEvent -> onLogin (me/leoko/advancedban/bungee/listener/ConnectionListenerBungee.java) **no SDK event of this name**
- PubSubMessageEvent -> onMessageReceive (me/leoko/advancedban/bungee/listener/PubSubMessageListener.java) **no SDK event of this name**
- PunishmentEvent -> onPunish (me/leoko/advancedban/bukkit/listener/InternalListener.java) **no SDK event of this name**
- PunishmentEvent -> onPunish (me/leoko/advancedban/bungee/listener/InternalListener.java) **no SDK event of this name**
- RevokePunishmentEvent -> onRevokePunishment (me/leoko/advancedban/bukkit/listener/InternalListener.java) **no SDK event of this name**
- RevokePunishmentEvent -> onUnPunish (me/leoko/advancedban/bungee/listener/InternalListener.java) **no SDK event of this name**
- TabCompleteEvent -> onTabComplete (me/leoko/advancedban/bungee/listener/ChatListenerBungee.java)

## Timers


## Files

- data/config.yml
- data/Layouts.yml
- data/Messages.yml
- data/simplelogger.properties

## Keys read from config / messages

- `calculation`
- `name`
- `operator`
- `punishmentType`
- `reason`
- `uuid`

## Biggest classes (write these first, they are most of the work)

- 24 KB me/leoko/advancedban/utils/Command.java
- 13 KB me/leoko/advancedban/bungee/BungeeMethods.java
- 12 KB me/leoko/advancedban/manager/PunishmentManager.java
- 12 KB me/leoko/advancedban/manager/UpdateManager.java
- 12 KB me/leoko/advancedban/utils/Punishment.java
- 12 KB me/leoko/advancedban/bukkit/BukkitMethods.java
- 10 KB me/leoko/advancedban/Universal.java
- 6 KB me/leoko/advancedban/manager/UUIDManager.java
- 6 KB me/leoko/advancedban/utils/commands/PunishmentProcessor.java
- 6 KB me/leoko/advancedban/manager/DatabaseManager.java
- 4 KB me/leoko/advancedban/utils/SQLQuery.java
- 4 KB me/leoko/advancedban/utils/commands/ListProcessor.java
- 3 KB me/leoko/advancedban/bungee/listener/InternalListener.java
- 3 KB me/leoko/advancedban/manager/LogManager.java
- 2 KB me/leoko/advancedban/bungee/listener/ConnectionListenerBungee.java
- 2 KB me/leoko/advancedban/manager/MessageManager.java
- 2 KB me/leoko/advancedban/MethodInterface.java
- 2 KB me/leoko/advancedban/utils/PunishmentType.java
- 2 KB me/leoko/advancedban/utils/tabcompletion/PunishmentTabCompleter.java
- 1 KB me/leoko/advancedban/bukkit/listener/ConnectionListener.java
- 1 KB me/leoko/advancedban/bungee/listener/PubSubMessageListener.java
- 1 KB me/leoko/advancedban/bungee/BungeeMain.java
- 1 KB me/leoko/advancedban/utils/DynamicDataSource.java
- 1 KB me/leoko/advancedban/utils/CommandUtils.java
- 1 KB me/leoko/advancedban/bukkit/listener/InternalListener.java

## Kit map

- messages: `msg("key", &[args])` (Lang: {prefix}, %s, {0}); settings: `config(|c| c.int("path", default))`
- commands: the Router above; a sub = `fn(&Ctx) -> Res` (`c.player()?`, `c.int(0, "amount")?`, `c.online(0)?`, `c.tell(..)`)
- client-only entities (holograms, markers, NPCs): `horizon_kit::packets` (+ `wire` for raw bytes)
- per-tick work: `horizon_kit::worker` (server-thread cost ~0, as Java async)
- a `/x profiler`: `horizon_kit::stats::{set_running, report, timed}`
- any Bukkit call: `mcp plugin-port api_lookup <Class.method>` / `sdk_search`
