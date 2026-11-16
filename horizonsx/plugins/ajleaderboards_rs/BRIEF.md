# ajLeaderboards 2.11.0-b339: rewrite brief

Main class `us.ajg0702.leaderboards.LeaderboardPlugin`. 126 own classes, 473 KB of Java (java/).

## Commands

- `/ajleaderboards` aliases ['ajlb', 'ajl', 'alb'], permission `ajleaderboards.use`: Commands for ajLeaderboards

## Sub-commands found in the source

- none found: read the command classes

## Permissions

- `ajleaderboards.use`

## Events

- PlayerJoinEvent -> onJoin (us/ajg0702/leaderboards/Listeners.java)
- PlayerQuitEvent -> onQuit (us/ajg0702/leaderboards/Listeners.java)

## Timers

- runTaskTimerAsynchronously delay 0 period 18000 (us/ajg0702/leaderboards/LeaderboardPlugin.java)
- runTaskTimerAsynchronously delay 100 period 600 (us/ajg0702/leaderboards/LeaderboardPlugin.java)
- runTaskLaterAsynchronously delay 2400 period  (us/ajg0702/leaderboards/LeaderboardPlugin.java)
- runTaskTimerAsynchronously delay 72000 period 2592000 (us/ajg0702/leaderboards/LeaderboardPlugin.java)
- runTaskLaterAsynchronously delay 40 period  (us/ajg0702/leaderboards/Listeners.java)

## Files

- data/cache_storage.yml
- data/config.yml
- data/slimjar.json

## Keys read from config / messages

- `COMMENT`
- `REMARKS`
- `blocking-fetch`
- `bytebin-link`
- `characterEncoding`
- `comma`
- `database`
- `decimal`
- `displaynamecache`
- `formatted.b`
- `formatted.d`
- `formatted.k`
- `formatted.m`
- `formatted.n`
- `formatted.o`
- `formatted.q`
- `formatted.qi`
- `formatted.sp`
- `formatted.sx`
- `formatted.t`
- `formatted.ud`
- `id`
- `ip`
- `loading.position`
- `loading.size`
- `method`
- `namecache`
- `no-data-name`
- `no-data-score`
- `password`
- `prefixcache`
- `reset-weekly-on`
- `serverUuid`
- `suffixcache`
- `table_prefix`
- `username`
- `web-link`

## Biggest classes (write these first, they are most of the work)

- 75 KB us/ajg0702/leaderboards/cache/Cache.java
- 29 KB us/ajg0702/leaderboards/boards/TopManager.java
- 27 KB us/ajg0702/leaderboards/LeaderboardPlugin.java
- 25 KB us/ajg0702/leaderboards/cache/methods/MysqlMethod.java
- 25 KB us/ajg0702/leaderboards/utils/Metrics.java
- 15 KB us/ajg0702/leaderboards/boards/StatEntry.java
- 10 KB us/ajg0702/leaderboards/cache/ExtraManager.java
- 8 KB us/ajg0702/leaderboards/cache/methods/H2Method.java
- 8 KB us/ajg0702/leaderboards/utils/UnClosableConnection.java
- 7 KB us/ajg0702/leaderboards/displays/armorstands/ArmorStandManager.java
- 7 KB us/ajg0702/leaderboards/displays/signs/SignManager.java
- 7 KB us/ajg0702/leaderboards/cache/methods/SqliteMethod.java
- 7 KB us/ajg0702/leaderboards/nms/legacy/HeadUtils.java
- 6 KB us/ajg0702/leaderboards/placeholders/PlaceholderExpansion.java
- 6 KB us/ajg0702/leaderboards/displays/heads/HeadManager.java
- 6 KB us/ajg0702/leaderboards/cache/helpers/DbRow.java
- 6 KB us/ajg0702/leaderboards/commands/base/platforms/bukkit/BukkitSender.java
- 5 KB us/ajg0702/leaderboards/commands/main/subcommands/signs/AddSign.java
- 5 KB us/ajg0702/leaderboards/displays/lpcontext/PositionContext.java
- 5 KB us/ajg0702/leaderboards/loaders/MessageLoader.java
- 4 KB us/ajg0702/leaderboards/commands/main/subcommands/Import.java
- 4 KB us/ajg0702/leaderboards/displays/signs/BoardSign.java
- 4 KB us/ajg0702/leaderboards/commands/main/subcommands/Viewer.java
- 4 KB us/ajg0702/leaderboards/formatting/formats/Time.java
- 4 KB us/ajg0702/leaderboards/commands/main/subcommands/RemovePlayer.java

## Kit map

- messages: `msg("key", &[args])` (Lang: {prefix}, %s, {0}); settings: `config(|c| c.int("path", default))`
- commands: the Router above; a sub = `fn(&Ctx) -> Res` (`c.player()?`, `c.int(0, "amount")?`, `c.online(0)?`, `c.tell(..)`)
- client-only entities (holograms, markers, NPCs): `horizon_kit::packets` (+ `wire` for raw bytes)
- per-tick work: `horizon_kit::worker` (server-thread cost ~0, as Java async)
- a `/x profiler`: `horizon_kit::stats::{set_running, report, timed}`
- any Bukkit call: `mcp plugin-port api_lookup <Class.method>` / `sdk_search`
