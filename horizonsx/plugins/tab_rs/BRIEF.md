# TAB 6.2.1: rewrite brief

Main class `me.neznamy.tab.platforms.bukkit.BukkitTAB`. 549 own classes, 2237 KB of Java (java/).

## Commands

- `/tab`: Plugin's main command

## Sub-commands found in the source

- `download` (me/neznamy/tab/shared/command/MySQLCommand.java:34)
- `me` (me/neznamy/tab/shared/command/ParseCommand.java:29)
- `upload` (me/neznamy/tab/shared/command/MySQLCommand.java:40)

## Permissions

- `tab.admin`
- `tab.announce.bar`
- `tab.announce.scoreboard`
- `tab.bossbar.toggle`
- `tab.bypass`
- `tab.change.*`
- `tab.cpu`
- `tab.dump`
- `tab.group.*`
- `tab.groupinfo`
- `tab.grouplist`
- `tab.mysql.download`
- `tab.mysql.upload`
- `tab.nametag.toggle`
- `tab.nametag.toggle.other`
- `tab.parse`
- `tab.reload`
- `tab.remove`
- `tab.scoreboard.show`
- `tab.scoreboard.show.other`
- `tab.scoreboard.toggle`
- `tab.scoreboard.toggle.other`
- `tab.seevanished`
- `tab.send.bar`
- `tab.setcollision`
- `tab.spectatorbypass`
- `tab.staff`
- `tab.tabcomplete`

## Events

- PlayerChangedWorldEvent -> onWorldChange (me/neznamy/tab/platforms/bukkit/features/PerWorldPlayerList.java)
- PlayerDisconnectEvent -> onQuit (me/neznamy/tab/platforms/bungeecord/BungeeEventListener.java) **no SDK event of this name**
- PlayerJoinEvent -> onJoin (me/neznamy/tab/platforms/bukkit/features/PerWorldPlayerList.java)
- PluginMessageEvent -> onPluginMessage (me/neznamy/tab/platforms/bungeecord/BungeeEventListener.java) **no SDK event of this name**
- ServerSwitchEvent -> onSwitch (me/neznamy/tab/platforms/bungeecord/BungeeEventListener.java) **no SDK event of this name**

## Timers


## Files

- data/config/animations.yml
- data/config/config.yml
- data/config/groups.yml
- data/config/messages.yml
- data/config/users.yml
- data/rabbitmq-amqp-client.properties
- data/version.properties
- data/fand-plugin.json

## Keys read from config / messages

- `bossbar-already-announced`
- `bossbar-announce-command-usage`
- `bossbar-announcement-success`
- `bossbar-feature-not-enabled`
- `bossbar-not-found`
- `bossbar-send-success`
- `bossbar-toggle-command`
- `bossbar-toggle-off`
- `bossbar-toggle-on`
- `channel-name-suffix`
- `collision-command-usage`
- `color`
- `command-only-from-game`
- `condition`
- `database`
- `date-format`
- `default-skin`
- `direction`
- `disable-condition`
- `display-condition`
- `exchange`
- `fancy-value`
- `fancy-value-default`
- `group`
- `group-data-removed`
- `group-value-assigned`
- `group-value-removed`
- `host`
- `invalid-number`
- `locale`
- `messages.yml`
- `multi-world-separator`
- `mysql-download-success`
- `mysql-fail-error`
- `mysql-fail-not-enabled`
- `mysql-upload-success`
- `name`
- `nametag.feature-not-enabled`
- `nametag.opaque-player-enabled`
- `nametag.opaque-view-enabled`
- `nametag.player-hidden`
- `nametag.player-shown`
- `nametag.view-hidden`
- `nametag.view-shown`
- `parse-command-usage`
- `password`
- `player-not-online`
- `playerlist-objective.value`
- `property`
- `reload-success`
- `remaining-players-text`
- `render-type`
- `scoreboard-not-found`
- `scoreboard-show-usage`
- `scoreboard-toggle-off`
- `scoreboard-toggle-on`
- `send-bar-command-usage`
- `server`
- `skin`
- `sorting-placeholder`
- `sorting-type`
- `style`
- `team-feature-required`
- `text`
- `time-format`
- `title`
- `toggle-command`
- `type`
- `url`
- `user`
- `user-data-removed`
- `user-value-assigned`
- `user-value-removed`
- `username`
- `value`
- `world`
- `yellow-number-in-tablist`

## Biggest classes (write these first, they are most of the work)

- 33 KB me/neznamy/tab/shared/platform/decorators/SafeScoreboard.java
- 32 KB me/neznamy/tab/shared/config/converter/LegacyConverter.java
- 29 KB me/neznamy/tab/shared/features/PlaceholderManagerImpl.java
- 28 KB me/neznamy/tab/shared/features/nametags/NameTag.java
- 22 KB me/neznamy/tab/shared/FeatureManager.java
- 22 KB me/neznamy/tab/shared/features/scoreboard/ScoreboardManagerImpl.java
- 21 KB me/neznamy/tab/shared/features/nametags/VisibilityManager.java
- 20 KB me/neznamy/tab/shared/features/playerlistobjective/YellowNumber.java
- 20 KB me/neznamy/tab/shared/features/belowname/BelowName.java
- 19 KB me/neznamy/tab/shared/features/playerlist/PlayerList.java
- 19 KB me/neznamy/tab/platforms/bukkit/platform/BukkitPlatform.java
- 17 KB me/neznamy/tab/shared/chat/component/TabComponent.java
- 17 KB me/neznamy/tab/shared/features/bossbar/BossBarLine.java
- 16 KB me/neznamy/tab/shared/config/MessageFile.java
- 16 KB me/neznamy/tab/shared/features/globalplayerlist/GlobalPlayerList.java
- 16 KB me/neznamy/tab/shared/features/layout/LayoutConfiguration.java
- 16 KB me/neznamy/tab/shared/features/bossbar/BossBarManagerImpl.java
- 14 KB me/neznamy/tab/shared/config/files/Config.java
- 14 KB me/neznamy/tab/platforms/velocity/VelocityPlatform.java
- 13 KB me/neznamy/tab/platforms/bungeecord/BungeePlatform.java
- 12 KB me/neznamy/tab/platforms/bukkit/v1_12_R1/NMSPacketTabList.java
- 12 KB me/neznamy/tab/shared/TabConstants.java
- 12 KB me/neznamy/tab/platforms/bukkit/v1_16_R3/NMSPacketTabList.java
- 12 KB me/neznamy/tab/shared/platform/TabPlayer.java
- 12 KB me/neznamy/tab/platforms/bukkit/v1_21_R7/NMSPacketTabList.java

## Kit map

- messages: `msg("key", &[args])` (Lang: {prefix}, %s, {0}); settings: `config(|c| c.int("path", default))`
- commands: the Router above; a sub = `fn(&Ctx) -> Res` (`c.player()?`, `c.int(0, "amount")?`, `c.online(0)?`, `c.tell(..)`)
- client-only entities (holograms, markers, NPCs): `horizon_kit::packets` (+ `wire` for raw bytes)
- per-tick work: `horizon_kit::worker` (server-thread cost ~0, as Java async)
- a `/x profiler`: `horizon_kit::stats::{set_running, report, timed}`
- any Bukkit call: `mcp plugin-port api_lookup <Class.method>` / `sdk_search`
