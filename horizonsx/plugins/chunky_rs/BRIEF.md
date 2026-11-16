# Chunky 1.5.3: rewrite brief

Main class `org.popcraft.chunky.ChunkyBukkit`. 119 own classes, 277 KB of Java (java/).

## Commands

- `/chunky` aliases ['cy'], permission `chunky.command`: Generates chunks

## Sub-commands found in the source

- none found: read the command classes

## Permissions

- `chunky.chunky`
- `chunky.command`
- `chunky.command.border`
- `chunky.command.cancel`
- `chunky.command.center`
- `chunky.command.confirm`
- `chunky.command.continue`
- `chunky.command.corners`
- `chunky.command.help`
- `chunky.command.pattern`
- `chunky.command.pause`
- `chunky.command.progress`
- `chunky.command.quiet`
- `chunky.command.radius`
- `chunky.command.reload`
- `chunky.command.selection`
- `chunky.command.shape`
- `chunky.command.silent`
- `chunky.command.spawn`
- `chunky.command.start`
- `chunky.command.trim`
- `chunky.command.world`
- `chunky.command.worldborder`

## Events

- RegionizedServerInitEvent -> onRegionisedServerInit (org/popcraft/chunky/platform/Folia.java)
- WorldInitEvent -> onWorldInit (org/popcraft/chunky/ChunkyBukkit.java)

## Timers


## Files

- data/config.yml
- data/version.properties

## Keys read from config / messages

- `language`

## Biggest classes (write these first, they are most of the work)

- 34 KB org/popcraft/chunky/util/Hilbert.java
- 14 KB org/popcraft/chunky/command/TrimCommand.java
- 11 KB org/popcraft/chunky/GenerationTask.java
- 8 KB org/popcraft/chunky/iterator/RegionChunkIterator.java
- 8 KB org/popcraft/chunky/platform/BukkitWorld.java
- 7 KB org/popcraft/chunky/iterator/WorldChunkIterator.java
- 7 KB org/popcraft/chunky/Chunky.java
- 6 KB org/popcraft/chunky/util/TaskLoader.java
- 6 KB org/popcraft/chunky/util/TranslationKey.java
- 6 KB org/popcraft/chunky/ChunkyBukkit.java
- 6 KB org/popcraft/chunky/platform/impl/GsonConfig.java
- 5 KB org/popcraft/chunky/Selection.java
- 4 KB org/popcraft/chunky/command/StartCommand.java
- 4 KB org/popcraft/chunky/iterator/SpiralChunkIterator.java
- 4 KB org/popcraft/chunky/nbt/util/RegionFile.java
- 4 KB org/popcraft/chunky/nbt/CompoundTag.java
- 4 KB org/popcraft/chunky/platform/BukkitPlayer.java
- 4 KB org/popcraft/chunky/shape/Star.java
- 3 KB org/popcraft/chunky/util/Input.java
- 3 KB org/popcraft/chunky/api/ChunkyAPIImpl.java
- 3 KB org/popcraft/chunky/iterator/ConcentricChunkIterator.java
- 3 KB org/popcraft/chunky/command/WorldBorderCommand.java
- 2 KB org/popcraft/chunky/platform/Folia.java
- 2 KB org/popcraft/chunky/command/CancelCommand.java
- 2 KB org/popcraft/chunky/nbt/Tag.java

## Kit map

- messages: `msg("key", &[args])` (Lang: {prefix}, %s, {0}); settings: `config(|c| c.int("path", default))`
- commands: the Router above; a sub = `fn(&Ctx) -> Res` (`c.player()?`, `c.int(0, "amount")?`, `c.online(0)?`, `c.tell(..)`)
- client-only entities (holograms, markers, NPCs): `horizon_kit::packets` (+ `wire` for raw bytes)
- per-tick work: `horizon_kit::worker` (server-thread cost ~0, as Java async)
- a `/x profiler`: `horizon_kit::stats::{set_running, report, timed}`
- any Bukkit call: `mcp plugin-port api_lookup <Class.method>` / `sdk_search`
