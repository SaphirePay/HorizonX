# BetterRTP 3.6.13: rewrite brief

Main class `me.SuperRonanCraft.BetterRTP.BetterRTP`. 153 own classes, 380 KB of Java (java/).

## Commands

- `/betterrtp` aliases ['brtp', 'rtp', 'randomtp', 'wild', 'wildtp']: Randomly teleport to a location

## Sub-commands found in the source

- none found: read the command classes

## Permissions

- `betterrtp.*`
- `betterrtp.biome`
- `betterrtp.bypass.*`
- `betterrtp.bypass.cooldown`
- `betterrtp.bypass.delay`
- `betterrtp.bypass.economy`
- `betterrtp.bypass.hunger`
- `betterrtp.edit`
- `betterrtp.group.*`
- `betterrtp.info`
- `betterrtp.location`
- `betterrtp.player`
- `betterrtp.reload`
- `betterrtp.sign`
- `betterrtp.test`
- `betterrtp.updater`
- `betterrtp.use`
- `betterrtp.version`
- `betterrtp.world`
- `betterrtp.world.*`

## Events

- EntityDamageEvent -> event (me/SuperRonanCraft/BetterRTP/player/rtp/RTPDelay.java)
- EntityDamageEvent -> onDamage (me/SuperRonanCraft/BetterRTP/player/events/EventListener.java)
- InventoryClickEvent -> click (me/SuperRonanCraft/BetterRTP/player/events/EventListener.java)
- PlayerInteractEvent -> onInteract (me/SuperRonanCraft/BetterRTP/player/events/EventListener.java)
- PlayerJoinEvent -> onJoin (me/SuperRonanCraft/BetterRTP/player/events/EventListener.java)
- PlayerMoveEvent -> event (me/SuperRonanCraft/BetterRTP/player/rtp/RTPDelay.java)
- PlayerQuitEvent -> onLeave (me/SuperRonanCraft/BetterRTP/player/events/EventListener.java)
- PlayerRespawnEvent -> onRespawn (me/SuperRonanCraft/BetterRTP/player/events/EventListener.java)
- PlayerTeleportEvent -> teleport (me/SuperRonanCraft/BetterRTP/player/events/EventListener.java)
- RTP_TeleportPostEvent -> onRTP (me/SuperRonanCraft/BetterRTP/references/rtpinfo/QueueHandler.java) **no SDK event of this name**
- RTP_TeleportPostEvent -> rtpPost (me/SuperRonanCraft/BetterRTP/player/events/EventListener.java) **no SDK event of this name**
- SignChangeEvent -> interact (me/SuperRonanCraft/BetterRTP/player/events/EventListener.java)
- WorldLoadEvent -> worldLoad (me/SuperRonanCraft/BetterRTP/player/events/EventListener.java)

## Timers


## Files

- data/config.yml
- data/economy.yml
- data/effects.yml
- data/ideas.yml
- data/lang/br.yml
- data/lang/chs.yml
- data/lang/cht.yml
- data/lang/cs.yml
- data/lang/da.yml
- data/lang/de.yml
- data/lang/en.yml
- data/lang/es.yml
- data/lang/fr.yml
- data/lang/he.yml
- data/lang/hu.yml
- data/lang/it.yml
- data/lang/ja.yml
- data/lang/nl.yml
- data/lang/no.yml
- data/lang/pl.yml
- data/lang/ro.yml
- data/lang/ru.yml
- data/lang/tr.yml
- data/lang/uk.yml
- data/lang/vi.yml
- data/locations.yml
- data/permissions.yml
- data/placeholders.yml
- data/signs.yml
- data/mappings.json

## Keys read from config / messages

- `Config.CanRTP.Cooldown`
- `Config.CanRTP.Hunger`
- `Config.CanRTP.NoPermission`
- `Config.CanRTP.Price`
- `Config.CanRTP.Success`
- `Config.TimeFormat.Days`
- `Config.TimeFormat.Hours`
- `Config.TimeFormat.Infinite`
- `Config.TimeFormat.Minutes`
- `Config.TimeFormat.Seconds`
- `Config.TimeFormat.Separator.Last`
- `Config.TimeFormat.Separator.Middle`
- `Config.TimeFormat.ZeroAll`
- `Language-File`
- `Messages.`
- `Messages.Prefix`
- `Particles.Shape`
- `Particles.Type`
- `Settings.Logger.Format`
- `Settings.RtpOnFirstJoin.World`
- `Sounds.Delay`
- `Sounds.Success`
- `Titles.`
- `serverUuid`

## Biggest classes (write these first, they are most of the work)

- 16 KB me/SuperRonanCraft/BetterRTP/player/commands/types/CmdEdit.java
- 14 KB me/SuperRonanCraft/BetterRTP/player/commands/types/CmdInfo.java
- 14 KB me/SuperRonanCraft/BetterRTP/player/rtp/packets/WrapperPlayServerNamedSoundEffect.java
- 9 KB me/SuperRonanCraft/BetterRTP/references/rtpinfo/worlds/WorldLocation.java
- 9 KB me/SuperRonanCraft/BetterRTP/references/helpers/HelperRTP_EditWorlds.java
- 8 KB me/SuperRonanCraft/BetterRTP/references/helpers/HelperRTP.java
- 8 KB me/SuperRonanCraft/BetterRTP/references/rtpinfo/worlds/WorldCustom.java
- 8 KB me/SuperRonanCraft/BetterRTP/references/depends/DepPlaceholderAPI.java
- 8 KB me/SuperRonanCraft/BetterRTP/references/rtpinfo/QueueGenerator.java
- 8 KB me/SuperRonanCraft/BetterRTP/references/database/SQLite.java
- 7 KB me/SuperRonanCraft/BetterRTP/references/rtpinfo/worlds/WorldPermissionGroup.java
- 7 KB me/SuperRonanCraft/BetterRTP/references/web/Metrics.java
- 7 KB me/SuperRonanCraft/BetterRTP/references/rtpinfo/worlds/WorldPlayer.java
- 6 KB me/SuperRonanCraft/BetterRTP/references/database/DatabaseQueue.java
- 6 KB me/SuperRonanCraft/BetterRTP/references/rtpinfo/RandomLocation.java
- 6 KB me/SuperRonanCraft/BetterRTP/references/rtpinfo/CooldownHandler.java
- 6 KB me/SuperRonanCraft/BetterRTP/references/settings/Settings.java
- 6 KB me/SuperRonanCraft/BetterRTP/player/rtp/RTPTeleport.java
- 5 KB me/SuperRonanCraft/BetterRTP/player/rtp/RTPLoader.java
- 5 KB me/SuperRonanCraft/BetterRTP/player/commands/types/CmdQueue.java
- 5 KB me/SuperRonanCraft/BetterRTP/BetterRTP.java
- 5 KB me/SuperRonanCraft/BetterRTP/references/messages/placeholder/PlaceholderAnalyzer.java
- 5 KB me/SuperRonanCraft/BetterRTP/references/rtpinfo/worlds/WorldDefault.java
- 4 KB me/SuperRonanCraft/BetterRTP/player/commands/types/CmdLocation.java
- 4 KB me/SuperRonanCraft/BetterRTP/player/rtp/effects/RTPEffect_Particles.java

## Kit map

- messages: `msg("key", &[args])` (Lang: {prefix}, %s, {0}); settings: `config(|c| c.int("path", default))`
- commands: the Router above; a sub = `fn(&Ctx) -> Res` (`c.player()?`, `c.int(0, "amount")?`, `c.online(0)?`, `c.tell(..)`)
- client-only entities (holograms, markers, NPCs): `horizon_kit::packets` (+ `wire` for raw bytes)
- per-tick work: `horizon_kit::worker` (server-thread cost ~0, as Java async)
- a `/x profiler`: `horizon_kit::stats::{set_running, report, timed}`
- any Bukkit call: `mcp plugin-port api_lookup <Class.method>` / `sdk_search`
