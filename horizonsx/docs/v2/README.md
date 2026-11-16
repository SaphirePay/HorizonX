# HorizonsX: the server for many players

HorizonsX is Horizon's dedicated server, tuned for player count. It is the same engine and the same
vanilla protocol, so Horizon clients and normal (vanilla) Minecraft Java clients join it as they join
Horizon. It lives on the `horizonsx` branch, in the worktree `D:/Horizon/v2`.

## What it changes

| Change | Vanilla behaviour | Switch |
|---|---|---|
| Batched socket writes: a tick's packets to a player go out as one write | same bytes, same order | always |
| Spawning finds the players near a chunk by a grid, not a scan of all of them | same | always |
| A chunk batch reads each neighbour's sections once | same | always |
| Fx hasher (rustc's) on the scheduled-tick queues and the biome spawn lists | same | always |
| Entity activation range (Spigot's, on in Paper): a mob over 32 blocks from every player, on the ground with nothing happening to it, ticks one tick in eight | differs (as Paper) | `HORIZON_ACTIVATION=0` turns it off; a number sets the range |
| Natural spawning by regions: players whose spawn areas touch form a region, regions are over 128 blocks apart, each spawns on its own core with its own random | rules the same, random draws differ | `HORIZON_REGIONS=0`: vanilla's single pass. One region always runs vanilla's pass |
| Mob ticks by regions: mobs in 128-block cells that touch tick together, groups far apart tick on their own cores | rules the same, random draws and the order across groups differ | `HORIZON_REGIONS=0`; under 256 mobs, one group, or a content mod's effect on a mob: the serial loop |
| Chunk columns sent to players encoded on several cores | same bytes | `HORIZON_REGIONS=0`; under 8 columns a tick: one core |
| Each player's entity pair checks (who sees and tracks what) on several cores | same packets, same order | `HORIZON_REGIONS=0`; under 16 players or 256 entities: one core |
| Random tick positions picked on several cores (each chunk its own draws); the ticks run on the game thread in chunk order | rules the same, random draws differ | `HORIZON_REGIONS=0` |
| Item merge radius 2.5 blocks (Spigot's and Paper's default) | differs (as Paper) | `HORIZON_ITEM_MERGE=0.5`: vanilla's |
| One work queue for every parallel phase (Folia's tick threads): items claimed largest first by whichever thread is free, the tick's own first and among them | same | always |
| Block ticks by region (Folia's region threading): ticking chunks in 4×4-chunk sections, sections two apart one region; each region's chunks, block entities, block/fluid tick lists and random-tick cache lent to it, its scheduled, fluid and random ticks on its own core, everything handed back after. The largest region ticks on the level's own data. A chunk outside a region reads as unloaded to it (Folia's rule) | rules the same, order across regions and random draws differ | `HORIZON_REGIONS=0`; off while a plugin listens on block hooks |
| Spawn tiles: a spawn region with more than its share of the cores spawns by 8×8-chunk tiles in nine rounds (a round's tiles 256+ blocks apart, so local mob caps are exact); spawns committed in order against the global caps | rules the same, random draws differ | `HORIZON_REGIONS=0` |
| Item entities: each one's own tick (moves, what its block does to it) on every core, then merges, removals and sounds in vanilla's order | a merge sees the other items after their moves | under 256 items: serial |
| Locator bar: each receiver's waypoint row indexed by source (vanilla scans it: a cube of the player count a tick) | same packets | always |
| Mob tiles (beyond Folia): a region with over twice its share of the mobs (players' trails joined their bases into one) ticks by its 128-block cells in four rounds (a 2×2 pattern: no two cells of a round touch; each reads its neighbours' mobs as the rounds before left them); the other regions tick whole in the first round | rules the same, order and random draws differ | `HORIZON_MOB_TILES=0`; measured 11-04, 100 players with explorers (one region of ~5,000 mobs): mob entities 44.8 -> 27.5 ms |

What runs in parallel only reads what the compiler can prove thread-safe (`MobWorldParts`,
`SpawnShared`, `Look`); the engine still forbids `unsafe`. Every parallel phase runs on one pool of
worker threads (the cores but one, `HORIZON_WORKERS`), started once.

Parallel is the default: on the pool every phase beat its serial loop. `HORIZON_PARALLEL=auto`
instead times each phase per unit of work in both modes and uses the faster, trying the other again
every 50 runs, for a machine shared with other heavy programs (Ember's "workers by measured load").

## Paper plugins

`python tools/v2/paper_build.py NAME` builds HorizonsX with the Paper engine's plugin layer (D:/Horizon/paper-engine,
no Fabric in it): its bundle `paper_api`, Dim patch and family rows, as its `build_engine.py` applies them,
applied to this worktree. Plugin DLLs go in the server's `plugins` folder with their `plugin.yml`, in
Paper's load order, as on Paper. The Paper engine's files are only read.

- The port's speed and RAM rows are HorizonsX's already (in HorizonsX's own form); they are skipped.
- `tools/v2/paper_v2_rows.py` fills in HorizonsX's hook points. A multi-core part gives way to plugins:
  mob ticks run on the game thread while a plugin listens to any mob event (`ee_*`, damage, effect
  changes), natural spawning while one listens to `PreCreatureSpawnEvent`, and random ticks stop
  between chunks to answer a plugin's say in a block change, as the serial loop does.
- The serial mob and random-tick loops stay as the base commit wrote them, so the port's rows find
  them; the multi-core paths run first and leave them nothing to tick.

## Measured (11-15, the build in this repository)

**Test PC:** AMD Ryzen 7 7800X3D (8 cores, 16 threads), 64 GB of RAM, NVIDIA GeForce RTX 3060 Ti, Windows 11. The
bots ran on the same PC, and the numbers are the server's own report. Runs were repeated whenever another program
compiled during them.

**Setup:** `horizonsx-region-threads=false` (one clock: see below), walking bots in groups of 5 at homes 512 blocks
apart on a pre-generated world, mob spawning as on a live survival server (monster cap 45, spawn attempts every 2
ticks), 60 s warm-up, 20 s to settle, then 15 s measured:
`python -I tools/v2/ramp.py NAME --mode spread --group 5 --start 300 --step 50 --max 500`.

| Players | TPS | ms a tick (average) | p99 | Worst tick | RAM | CPU in use |
|---|---|---|---|---|---|---|
| 300 | 20.0 | 28.3 | 59 ms | 111 ms | 3.7 GB | 5.5 cores |
| 400 | 20.0 | 37.9-38.5 | 63-82 ms | 141-169 ms | 4.4 GB | 7.3-7.5 cores |
| 450 | 20.0 | 45.6 | 139 ms | 193 ms | 5.0 GB | 8.5 cores |
| 500 | 19.2 | 51.9 | 96 ms | 202 ms | 5.4 GB | 9.3 cores |

The 400-player row is the range of four runs. A tick has 50 ms; 400 players leave about 12 ms of it spare.

**One clock** (`horizonsx-region-threads=false` in `server.properties`): the whole server keeps one TPS, as on Paper,
and no part of the world runs ahead or falls behind. Each tick's work still goes over the cores, as Pumpkin does it:
the level's regions are ticked by a few region games at once (three quarters of the hardware threads: 12 on the
7800X3D), and every tick waits for all of them. This is the mode measured above. With region threads on, each
region keeps its own TPS (Folia's model). `HORIZON_REGION_THREADS=0` is the one-thread server.

Where the 400-player tick goes (about 38 ms): the slowest region game about 26 ms (natural spawning, block ticks,
mobs, entity tracking, chunks), plus about 10-12 ms of serial work on the main thread. Every 100 ticks the resident
regions land back on the main thread, which makes that one tick about 135 ms.

## Earlier measurements (11-04, an older build with region threads on)

Kept for comparison; the numbers above replace them.


100 players spread over the map (`bench.py NAME 100 --behaviour walk --spread 512:5`, 60 s warm-up, 60 s
measured): 20.0 TPS, 47 ms a tick, p99 69 ms, 1.55 GB. With a third of them flying non-stop over new land
(`--behaviour mixed`): 5.6 TPS (one connected region; Folia would sit on one core too). The tick's thread
is the limit: 1 worker vs 15 gave only +15%; the server used about 4 of 16 threads.

On the VPS (Linux, KVM, 10 vCPU capped at 8 cores, other services running; `tools/v2/vps_bench.py`, 11-04,
walking players spread 512:5, 60 s warm-up, 30 s measured), against the Rust Paper port running there:

| Server | Players | TPS | ms/tick | p99 | RAM | RAM/player |
|---|---|---|---|---|---|---|
| HorizonsX | 50 | 7.7 | 129 | 205 | 1.15 GB | 23 MB |
| Rust Paper port | 50 | 6.2 | 161 | 354 | 1.80 GB | 36 MB |
| HorizonsX | 100 | 6.8 | 147 | 286 | 1.79 GB | 18 MB |
| Rust Paper port | 100 | 5.5 | 183 | 340 | 2.44 GB | 24 MB |

The VPS's cores are about three times slower than the 7800X3D's: what bounds the tick there is the same tick-thread
time, so every millisecond taken off it on the PC counts three times on the VPS.

Other changes measured by profile (100 walking players, 11-04): conduits looked for in a fortieth of the chunks
each tick instead of all of them every 40 ticks (that tick was 16-20 ms longer); a ticking minecart's rolled loot
lent, not cloned (~4%); the wardens and allays that hear game events kept per tick instead of found among every
mob for each event; the serial mob loop's snapshot of every mob made only when that loop runs (~3%).

Short runs (`tools/v2/ab.py ... --warmup 20 --seconds 30`) measure the terrain burst right after the bots
arrive and read lower (the same build: 14.4 TPS short, 20.0 long): use them only to compare builds.

## Not done yet

- The players' tick: each guest runs through the single-player code in turn (`as_guest`), so it can't be split by
  thread without a rework. Measured 11-04 (profile of v2-f13s, 100 walking players): the guests' own tick is 4.1% of
  the tick's thread and the swaps in and out of the guests 1.7%. It is no longer the limit (it was ~25% before the
  other phases went to the pool); not worth the rework.
- Block ticks by region with a plugin on block hooks: needs a Folia-style rule for which thread a
  plugin's event runs on (until then those ticks stay on the game thread).
- The hub's entity states as a change log (docs/server-next.md W2), instead of every entity's
  state rebuilt each tick: measured 11-04 (v2-f12) at 3.0 ms of 43 (7%) with 100 walking players and 10 of 231
  (4%) with explorers. Not done: a `Mob` is not `Sync` (its goals are boxed trait objects), so the states can't be
  built on the pool without `unsafe`, which the engine forbids; and a change log needs every one of the hundreds of
  places that change a mob to mark it (a missed one is a client showing a stale mob), while the states already reuse
  last tick's buffers.
- Lighting after block edits off the game thread. Measured 11-04: settled, with 100 walking players, all of chunk
  streaming (lighting included) is 0.85 ms of a 43 ms tick (2%); in the burst right after 100 players arrive on new
  land, relighting edits (`Streaming::relight_edits`, mostly generation's writes into neighbouring chunks and fluids
  settling) is 5.8% of the tick's thread and building light jobs 2.5% (profile of v2-f13s). The light jobs
  themselves already run on the generation threads. Left for later: an asynchronous relight like vanilla's light
  engine (the job and replay log exist for whole-column light) would take up to ~8% off the arrival burst and ~2%
  off a settled server.

## Build and measure

```
python -I tools/v2/build.py NAME            # fast profile, cache D:/Horizon/v2-target, into D:/Horizon/perf/NAME
python -I tools/v2/build.py NAME --symbols  # the same code with line tables, for profiles
python -I tools/v2/bench.py NAME 50,100     # Horizon's server_bench with mixed bots, plus the server's phase report
python -I tools/v2/bench.py NAME 50 --profile 1000   # and samples of the server thread
python -I tools/v2/lines.py NAME [TEXT] [--within]   # hot source lines of a profile
python tools/v2/paper_build.py NAME          # HorizonsX with the Paper plugin layer (cache D:/Horizon/v2-paper-target)
python -I tools/v2/bench.py NAME 75 --mods D:/Horizon/v2-paper/plugins   # with Paper plugins on the server
python -I tools/v2/compare.py --quick 75 1 X=NAME:plugins PAPER=proto-engine5:plugins   # side by side
python -I tools/v2/bench.py NAME 100 --behaviour walk --spread 512:5 --warmup 60   # 100 players at their own bases
python -I tools/v2/ab.py OLD NEW NEW+HORIZON_WORKERS=1 100:walk:512:5 100:mixed:512:5   # back to back, waits for a quiet PC
```

Bench only on a quiet machine: parallel phases wait for cores another program holds, so a busy
machine makes them look slower than they are. Compare runs taken back to back.
