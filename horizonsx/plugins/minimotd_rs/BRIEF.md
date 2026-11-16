# MiniMOTD 2.2.5: rewrite brief

Main class `xyz.jpenilla.minimotd.paper.MiniMOTDPaper`. 22 own classes, 57 KB of Java (java/).

## Commands


## Sub-commands found in the source

- none found: read the command classes

## Permissions

- `minimotd.admin`

## Events

- RegionizedServerInitEvent -> updateCheck (xyz/jpenilla/minimotd/paper/MiniMOTDPaper.java)

## Timers


## Files

- data/license_minimotd.txt

## Keys read from config / messages


## Biggest classes (write these first, they are most of the work)

- 6 KB xyz/jpenilla/minimotd/common/config/MOTDConfig.java
- 5 KB xyz/jpenilla/minimotd/common/config/ConfigManager.java
- 5 KB xyz/jpenilla/minimotd/common/PingResponse.java
- 4 KB xyz/jpenilla/minimotd/common/config/PluginSettings.java
- 4 KB xyz/jpenilla/minimotd/common/CommandHandler.java
- 4 KB xyz/jpenilla/minimotd/common/config/PlayerCountModifier.java
- 3 KB xyz/jpenilla/minimotd/common/MiniMOTD.java
- 3 KB xyz/jpenilla/minimotd/paper/MiniMOTDPaper.java
- 3 KB xyz/jpenilla/minimotd/common/config/ConfigLoader.java
- 3 KB xyz/jpenilla/minimotd/common/util/Nag.java
- 2 KB xyz/jpenilla/minimotd/common/IconManager.java
- 2 KB xyz/jpenilla/minimotd/common/util/UpdateChecker.java
- 1 KB xyz/jpenilla/minimotd/paper/MiniMOTDStub.java
- 1 KB xyz/jpenilla/minimotd/common/util/BrigadierUtil.java
- 1 KB xyz/jpenilla/minimotd/paper/PingListener.java
- 1 KB xyz/jpenilla/minimotd/common/util/MiniPlaceholdersUtil.java
- 1 KB xyz/jpenilla/minimotd/common/util/Pair.java
- 0 KB xyz/jpenilla/minimotd/common/util/ComponentColorDownsampler.java
- 0 KB xyz/jpenilla/minimotd/common/Constants.java
- 0 KB xyz/jpenilla/minimotd/common/MiniMOTDPlatform.java
- 0 KB xyz/jpenilla/minimotd/common/config/PostProcessor.java
- 0 KB xyz/jpenilla/minimotd/common/util/Int2IntFunction.java

## Kit map

- messages: `msg("key", &[args])` (Lang: {prefix}, %s, {0}); settings: `config(|c| c.int("path", default))`
- commands: the Router above; a sub = `fn(&Ctx) -> Res` (`c.player()?`, `c.int(0, "amount")?`, `c.online(0)?`, `c.tell(..)`)
- client-only entities (holograms, markers, NPCs): `horizon_kit::packets` (+ `wire` for raw bytes)
- per-tick work: `horizon_kit::worker` (server-thread cost ~0, as Java async)
- a `/x profiler`: `horizon_kit::stats::{set_running, report, timed}`
- any Bukkit call: `mcp plugin-port api_lookup <Class.method>` / `sdk_search`
