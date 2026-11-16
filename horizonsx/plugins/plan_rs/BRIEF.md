# Plan 5.8 build 3638: rewrite brief

Main class `com.djrapitops.plan.Plan`. 1448 own classes, 4693 KB of Java (java/).

## Commands

- `/plan`, permission `plan.command`

## Sub-commands found in the source

- none found: read the command classes

## Permissions

- `plan.command`
- `plan.data.backup`
- `plan.data.base`
- `plan.data.clear`
- `plan.data.export`
- `plan.data.hotswap`
- `plan.data.import`
- `plan.data.merge`
- `plan.data.move`
- `plan.data.remove.player`
- `plan.data.remove.server`
- `plan.data.restore`
- `plan.disable`
- `plan.ignore.afk`
- `plan.ignore.commanduse`
- `plan.info`
- `plan.ingame.other`
- `plan.ingame.self`
- `plan.json.other`
- `plan.json.self`
- `plan.logout.other`
- `plan.network`
- `plan.player.other`
- `plan.player.self`
- `plan.register.other`
- `plan.register.self`
- `plan.reload`
- `plan.search`
- `plan.server`
- `plan.servers`
- `plan.unregister.other`
- `plan.unregister.self`
- `plan.users`

## Events

- PlayerJoinEvent -> onPlayerJoin (com/djrapitops/plan/gathering/timed/BukkitPingCounter.java)
- PlayerJoinEvent -> onPlayerJoin (com/djrapitops/plan/gathering/timed/NukkitPingCounter.java)
- PlayerQuitEvent -> onPlayerQuit (com/djrapitops/plan/gathering/timed/BukkitPingCounter.java)
- PlayerQuitEvent -> onPlayerQuit (com/djrapitops/plan/gathering/timed/NukkitPingCounter.java)
- ServerConnectedEvent -> onPlayerJoin (com/djrapitops/plan/gathering/timed/BungeePingCounter.java) **no SDK event of this name**
- ServerDisconnectEvent -> onPlayerQuit (com/djrapitops/plan/gathering/timed/BungeePingCounter.java) **no SDK event of this name**

## Timers

- runTaskTimerAsynchronously delay 1200 period  (com/djrapitops/plan/storage/database/SQLiteDB.java)

## Files

- data/nukkit.yml
- data/plan-sponge.mixins.json
- data/assets/plan/AssetVersion.yml
- data/assets/plan/DefaultServerInfoFile.yml
- data/assets/plan/bungeeconfig.yml
- data/assets/plan/config.yml

## Keys read from config / messages

- `COLUMN_NAME`
- `CONSTRAINT_NAME`
- `GraphColors.WorldPie`
- `Key_name`
- `REFERENCED_COLUMN_NAME`
- `REFERENCED_TABLE_NAME`
- `Server.UUID`
- `Server.Web_address`
- `TABLE_NAME`
- `activity_group`
- `additional_info`
- `address`
- `banned`
- `code`
- `col_1_name`
- `col_2_name`
- `col_3_name`
- `col_4_name`
- `col_5_name`
- `color`
- `component_value`
- `cookie`
- `description`
- `element_order`
- `family`
- `format_type`
- `from_ip`
- `geolocation`
- `group_name`
- `group_value`
- `icon_name`
- `ip_address`
- `join_address`
- `killer_name`
- `killer_uuid`
- `linked_to_uuid`
- `name`
- `nickname`
- `nicknames`
- `permission`
- `plan_version`
- `plugin_name`
- `preferences`
- `provider_icon_color`
- `provider_icon_family`
- `provider_icon_name`
- `provider_name`
- `provider_text`
- `request_method`
- `request_uri`
- `s_uuid`
- `salted_pass_hash`
- `salted_password_hash`
- `server_name`
- `server_uuid`
- `string_value`
- `tab_icon_color`
- `tab_icon_family`
- `tab_icon_name`
- `tab_name`
- `table_color`
- `table_name`
- `text`
- `user_permissions`
- `username`
- `uuid`
- `version`
- `victim_name`
- `victim_uuid`
- `weapon`
- `web_address`
- `world`
- `world_name`

## Biggest classes (write these first, they are most of the work)

- 148 KB com/djrapitops/plan/DaggerPlanBukkitComponent.java
- 144 KB com/djrapitops/plan/DaggerPlanNukkitComponent.java
- 142 KB com/djrapitops/plan/DaggerPlanSpongeComponent.java
- 135 KB com/djrapitops/plan/DaggerPlanVelocityComponent.java
- 135 KB com/djrapitops/plan/DaggerPlanBungeeComponent.java
- 52 KB com/djrapitops/plan/storage/database/queries/objects/SessionQueries.java
- 50 KB com/djrapitops/plan/settings/locale/lang/HtmlLang.java
- 40 KB com/djrapitops/plan/storage/database/queries/objects/TPSQueries.java
- 37 KB com/djrapitops/plan/storage/database/queries/LargeStoreQueries.java
- 31 KB com/djrapitops/plan/extension/implementation/providers/gathering/DataValueGatherer.java
- 29 KB com/djrapitops/plan/processing/processors/move/DatabaseCopyProcessor.java
- 28 KB com/djrapitops/plan/placeholder/SessionPlaceHolders.java
- 26 KB com/djrapitops/plan/commands/subcommands/DatabaseCommands.java
- 24 KB com/djrapitops/plan/delivery/rendering/json/graphs/GraphJSONCreator.java
- 24 KB com/djrapitops/plan/commands/PlanCommand.java
- 23 KB com/djrapitops/plan/delivery/webserver/resolver/json/RootJSONResolver_Factory.java
- 22 KB com/djrapitops/plan/delivery/webserver/ResponseFactory.java
- 20 KB com/djrapitops/plan/delivery/rendering/json/JSONFactory.java
- 19 KB com/djrapitops/plan/extension/extractor/ExtensionExtractor.java
- 18 KB com/djrapitops/plan/placeholder/ServerPlaceHolders.java
- 18 KB com/djrapitops/plan/delivery/rendering/json/PlayerJSONCreator.java
- 17 KB com/djrapitops/plan/placeholder/PlayerPlaceHolders.java
- 16 KB com/djrapitops/plan/storage/database/SQLDB.java
- 16 KB com/djrapitops/plan/delivery/export/ServerPageExporter.java
- 15 KB com/djrapitops/plan/delivery/webserver/resolver/json/GraphsJSONResolver.java

## Kit map

- messages: `msg("key", &[args])` (Lang: {prefix}, %s, {0}); settings: `config(|c| c.int("path", default))`
- commands: the Router above; a sub = `fn(&Ctx) -> Res` (`c.player()?`, `c.int(0, "amount")?`, `c.online(0)?`, `c.tell(..)`)
- client-only entities (holograms, markers, NPCs): `horizon_kit::packets` (+ `wire` for raw bytes)
- per-tick work: `horizon_kit::worker` (server-thread cost ~0, as Java async)
- a `/x profiler`: `horizon_kit::stats::{set_running, report, timed}`
- any Bukkit call: `mcp plugin-port api_lookup <Class.method>` / `sdk_search`
