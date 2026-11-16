# HorizonsX and Folia: every feature, and where each one stands

The owner's order (2026-11-05): HorizonsX gets every feature Folia has, and its milliseconds per tick go as low
as they can. This file is the checklist; each line says what Folia does, what HorizonsX does today, and what is
left. Folia's source is in `D:/Horizon/ref/folia` (patches on Paper; the regionizer is
`0001-Region-Threading-Base.patch`).

Pass mark: 100 players spread over the map at a steady 20 TPS, the slowest 1% of ticks under 50 ms, on the
owner's PC and on the VPS (cores about three times slower).

## Baseline (2026-11-05, build `hx-0s` = commit d9e0cd9d1, quiet PC, 100 walking players 512 blocks apart)

42.2 ms a tick (p99 59), tick thread CPU 38 ms, 2.35 cores busy, 1.6 GB. The main thread's time, by phase:
mobs 14.3 ms (their region tick 8.7), network hub 11.9 (entity states 3.2, chunks 2.0, tracking 1.7), spawning
5.5, block ticks 5.5, block entities 3.5, sculk 2.9, random ticks 2.0. About 20% of the main thread is spent
waiting for the worker pool between phases. Tools: `tools/v2/tree.py NAME` (time tree of the tick thread) and
`tools/v2/callers.py NAME TEXT` (who calls a hot function), on `bench.py --profile` runs.

## The checklist

| # | Folia | HorizonsX (2026-11-05) | Left |
|---|---|---|---|
| 1 | Regions: loaded chunks grouped into regions (sections, a merge radius so regions never touch) | `game/regions.rs` `Regions`: 4×4-chunk sections; a section is active when it random-ticks or a player sees it (each player's whole view, Folia's rule); active sections 2 apart or closer are one region, which owns them and a section ring round them | - |
| 1a | Each region ticks on its own thread | Each region's world phases (blocks, entities, mobs, items, spawning, falling blocks, TNT, block entities) on a region game of its own on a thread of its own (`Flight`); what no region owns ticks on the main game meanwhile | - |
| 1b | Regions merge when players meet, split when they part | Worked out again every tick from the active sections, so they merge and split as players move | - |
| 2 | Lag isolation: a slow region doesn't hold the others | The tick waits for a region up to `HORIZON_REGION_DEADLINE_MS` (50: a region that still keeps 20 TPS is never cut loose); a later one keeps running on its own, its players' packets and leaves held, its chunks kept out of loading, and comes back after a later tick's world phases (its players get that tick's network turn). Saves and stops wait for every region. Test switch: `HORIZON_REGION_LAG_TEST_MS` | Proven in the end test |
| 3a | Chunk loading, generation and saving off the tick | Generation, lighting and disk on worker threads | - |
| 3b | Network off the world simulation; a player's connection ticked by its region | A reader and a writer thread per connection; the network tick's per-player parts on every core. Each region thread handles its own players' packets before its block ticks (moves, digging, placing, using, attacking, menus: `hub::region_local`); the first packet that reaches past the region (chat, commands, respawn, settings told to everyone) and the rest after it wait for the server thread, in order (`Guest::drained` carries the player's turn over). The anticheat runs on the region thread too (a copy of its rules; strikes under one lock) | - |
| 4 | Entities and AI per region | Every entity kind ticks in its region | - |
| 4a | Spigot/Paper entity activation range | Mobs far from every player, calm and on the ground (water mobs: in the water), tick one tick in eight: raiders past 48 blocks, water mobs past 16, the rest past 32 (`HORIZON_ACTIVATION` scales them, 0 turns it off); never a mob with a target, in love or growing up. Items keep vanilla's own idle step | - |
| 5 | Global region: time, weather, border, commands | The main game: players' maps, portals, raids, custom spawners, scoreboard, advancements, boss bars, the network's sending, sleep | - |
| 5a | Every level regionized | The End too once its dragon fight is over (`end_fight_active`; `HORIZON_END_REGIONS=0` keeps it one region) | - |
| 6 | `RegionScheduler`, `EntityScheduler`, `GlobalRegionScheduler`, `AsyncScheduler`, `teleportAsync` | In the plugin SDK (`paper-engine/sdk/horizon-bukkit` `fam_paper.rs`), with `isOwnedByCurrentRegion` and `isGlobalTickThread` | - |
| 7 | Plugins on the thread owning what they touch, several at once | Events fire on the region thread that did the work. A plugin whose `plugin.yml` says `folia-supported: true` (Folia's key) runs on every region thread at once; the rest take turns, one game thread at a time in their code (Folia refuses to load those at all) | - |
| 8 | `/tps` (per region), `/mspt`, `/profiler` | `game/health.rs`: `/tps` = TPS and MSPT over 5 s, 15 s, 1 m, 5 m, 15 m, the regions' last tick and the five busiest regions (place, chunks, players, ms), chunk loads and generations a second (Folia's throughput counters); `/mspt` = Paper's; `/profiler [s]` = a `horizon-report` of every phase. `horizonsx-commands=false` hides them | - |
| 9 | Region watchdog | A thread logs every region tick past 5 s, every 5 s: level, place, players, chunks and the phase it is in | - |
| 10 | Max pending logins | Paper's `max-joins-per-tick`: 20 joins a tick (`HORIZON_MAX_JOINS_PER_TICK`, 0 = no limit), a joiner's packets held behind its join | - |
| 11 | Folia's thread-safety guards (block updates into unowned chunks, worldgen reading in-world block entities, passenger desync, out-of-region teleport accept) | Covered by the design: a region's world holds only its chunks (a reach past them finds the chunk unloaded, Folia's rule), its edits past them are queued as spills for the server thread, generation works on its own proto chunks | - |
| - | Region threads switch | On; `HORIZON_REGION_THREADS=0` turns them off | - |

## The design

`Game` already keeps every per-level field in one movable bundle (`levels.rs` `Parked`: the world, block ticks,
entity lists, sculk, raids...). A Folia region is the same bundle for part of a level. So:

1. `Game` is `Send` (done 2026-11-05: `game_is_send` checks it at compile time). Four fields were in the way:
   the player's attribute map (`Rc<RefCell>` -> owned table), the identity allocator and the sign editor
   (`+ Send`), and the game's stored error (`HeldError`).
2. Worker games: one `Game` per worker thread, sharing the read-only resources with the main one.
3. Region data: a level's bundle split by region (the block-tick regions already lend chunks, block entities,
   tick lists and random-tick caches; entities are added).
4. A region's whole tick runs on a worker game with that region's data swapped in, the way `enter_level` swaps a
   dimension in today. The main thread keeps the global parts (Folia's global region).
5. Then each region gets its own schedule (lag isolation), then plugins.

Each step is measured at 100 players (ms per tick, p99, RAM) before and after, and recorded here.

When every line above is done: the same stress test on real Folia (Java), same PC, bots and settings, side by side (TPS, ms per tick, p99, RAM, RAM per player, CPU, players held at a steady 20 TPS). Folia may not have a 26.3 build; then both run the newest version they share.

## Region threads: how they are built (in progress)

- `game/regions.rs`: `Game::region_worker` makes a region game (`integrated::server_game` plus
  `copy_world_settings_from`); `Game.res_ref` lets it borrow the same resources. `Lan::detached` gives a
  region's hub the server's connections without the listener.
- `levels.rs` `swap_level_with`: the per-level fields between two games (kept in step with `swap_level`).
- `tools/v2/fields.py FN...` lists the `Game` fields a phase reaches (level or GLOBAL, reads and writes);
  `tools/v2/uses.py FIELD FN...` shows where. The world phases reach ~940 functions and write ~60 global
  fields besides the 67 a player's turn swaps (`enter_guest`) and the client-only ones.
- Policy per global field written from a region (Folia's): region-local copy merged back (event queues, entity
  id blocks per worker), read-only copy (rules, clock), or a task queued to the global region (the main
  thread) after the region ticks (streaming and forced chunks, other levels, the scoreboard, functions,
  boss bars, maps, commands that reach past the region).
- Entity ids: each region game draws blocks of ids from one shared counter (no two threads hand out the same).

## Log

- 2026-11-05: baseline above; `Game` is `Send`.
- 2026-11-05: mobs ticked in place by region, each mob's setup on every core, `Mob` is `Sync`: 42.2 -> 38.1 ms (p99 59 -> 52).
- 2026-11-05: found: one connected region (players' walking areas joined) holds 60% of the ticking chunks; its block ticks run on one thread. Folia has the same limit; tiles (as for mobs) go past it.
- 2026-11-05: region threads (`game/regions.rs`): 1024-block spread 69.5 -> 63.1 ms with 20 regions; lag isolation; regions own their players' views; on by default. Measured all together in the end test.

## HorizonsX against real Folia (2026-11-05, build hx-g4s, Folia 26.3 build 6 on Java 25)

Same PC (7800X3D), same bots (`horizon-bots`, walking), same settings and the same saved world (`perf/worlds/w512`),
`tools/v2/bench.py NAME BOTS --spread ... [--folia]`. Folia's tick times are its own: the probe plugin
(`tools/v2/folia-probe`, read-only) reads each region's per-tick times from Folia's scheduler; the busiest region's
are its tick. Folia ran with a 4 GB heap (`-Xms4G -Xmx4G`) and Folia's default region threads.

| Test | HorizonsX | Folia |
|---|---|---|
| 100 players 512 apart: TPS | **20.0** | 10.3 |
| ms per tick (average / p99) | **33.1 / 49.2** | 97.3 / 128.2 |
| RAM (RSS) / per player | **1.9 GB / 19 MB** | 4.9 GB / 49 MB |
| CPU (cores busy) | 3.1 | 2.5 |
| 100 players 1024 apart (many regions): TPS, ms, p99 | **20.0, 45.8, 72** | 18.9 (falling to 13), 52.9, 93 |
| 200 players 512 apart: TPS | **10.1** | 3.5 |
| Most players at a steady 20 TPS (512 apart) | **150** (47 ms average) | 50 (35 ms) |
| One region 80 ms slow each tick (lag isolation) | 20.0 TPS, 47.8 ms | - |
