# FancyNpcs 2.12.1+382: rewrite brief

Main class `de.oliver.fancynpcs.FancyNpcs`. 509 own classes, 1172 KB of Java (java/).

## Commands


## Sub-commands found in the source

- none found: read the command classes

## Permissions

- `FancyNpcs.admin`
- `fancynpcs.admin`
- `fancynpcs.bypass.cooldown`
- `fancynpcs.command.npc.action.add.*`

## Events

- NpcCreateEvent -> onNpcCreate (de/oliver/fancynpcs/listeners/PlayerNpcsListener.java) **no SDK event of this name**
- NpcModifyEvent -> onNpcModify (de/oliver/fancynpcs/listeners/PlayerNpcsListener.java) **no SDK event of this name**
- NpcRemoveEvent -> onNpcRemove (de/oliver/fancynpcs/listeners/PlayerNpcsListener.java) **no SDK event of this name**
- PlayerChangedWorldEvent -> onWorldChange (de/oliver/fancynpcs/listeners/PlayerChangedWorldListener.java)
- PlayerJoinEvent -> onPlayerJoin (de/oliver/fancynpcs/listeners/PlayerJoinListener.java)
- PlayerQuitEvent -> onPlayerQuit (de/oliver/fancynpcs/listeners/PlayerQuitListener.java)
- PlayerUseUnknownEntityEvent -> onPlayerUseUnknownEntity (de/oliver/fancynpcs/listeners/PlayerUseUnknownEntityListener.java)
- SkinGeneratedEvent -> onSkinGenerated (de/oliver/fancynpcs/skins/SkinManagerImpl.java) **no SDK event of this name**

## Timers


## Files

- data/version.yml
- data/languages/default.yml

## Keys read from config / messages

- `actions.`
- `attributes.`
- `creator`
- `displayName`
- `glowingColor`
- `location.world`
- `name`
- `skin.identifier`
- `skin.signature`
- `skin.uuid`
- `skin.value`
- `skin.variant`
- `type`
- `visibility`

## Biggest classes (write these first, they are most of the work)

- 26 KB de/oliver/fancynpcs/FancyNpcs.java
- 21 KB de/oliver/fancynpcs/v26_4/Npc_26_4.java
- 21 KB de/oliver/fancynpcs/v26_3/Npc_26_3.java
- 21 KB de/oliver/fancynpcs/v26_2/Npc_26_2.java
- 21 KB de/oliver/fancynpcs/v1_21_9/Npc_1_21_9.java
- 21 KB de/oliver/fancynpcs/v1_21_11/Npc_1_21_11.java
- 21 KB de/oliver/fancynpcs/v26_1_1/Npc_26_1_1.java
- 20 KB de/oliver/fancynpcs/v1_21_6/Npc_1_21_6.java
- 19 KB de/oliver/fancynpcs/NpcManagerImpl.java
- 19 KB de/oliver/fancynpcs/v1_21_5/Npc_1_21_5.java
- 14 KB de/oliver/fancynpcs/commands/oldCommands/CloudCommandManager.java
- 14 KB de/oliver/fancynpcs/commands/oldCommands/npc/ActionCMD.java
- 13 KB de/oliver/fancynpcs/commands/lampCommands/npc/ActionCMD.java
- 10 KB de/oliver/fancynpcs/api/NpcData.java
- 9 KB de/oliver/fancynpcs/FancyNpcsConfigImpl.java
- 9 KB de/oliver/fancynpcs/tests/commands/ActionCMDTest.java
- 9 KB de/oliver/fancynpcs/api/Npc.java
- 8 KB de/oliver/fancynpcs/skins/SkinManagerImpl.java
- 7 KB de/oliver/fancynpcs/tests/skins/SkinManagerTest.java
- 7 KB de/oliver/fancynpcs/commands/oldCommands/npc/EquipmentCMD.java
- 7 KB de/oliver/fancynpcs/commands/oldCommands/NpcConvertCMD.java
- 7 KB de/oliver/fancynpcs/commands/lampCommands/NpcConvertCMD.java
- 6 KB de/oliver/fancynpcs/tests/commands/CreateCMDTest.java
- 6 KB de/oliver/fancynpcs/commands/oldCommands/npc/SkinCMD.java
- 5 KB de/oliver/fancynpcs/commands/lampCommands/LampCommandManager.java

## Kit map

- messages: `msg("key", &[args])` (Lang: {prefix}, %s, {0}); settings: `config(|c| c.int("path", default))`
- commands: the Router above; a sub = `fn(&Ctx) -> Res` (`c.player()?`, `c.int(0, "amount")?`, `c.online(0)?`, `c.tell(..)`)
- client-only entities (holograms, markers, NPCs): `horizon_kit::packets` (+ `wire` for raw bytes)
- per-tick work: `horizon_kit::worker` (server-thread cost ~0, as Java async)
- a `/x profiler`: `horizon_kit::stats::{set_running, report, timed}`
- any Bukkit call: `mcp plugin-port api_lookup <Class.method>` / `sdk_search`
