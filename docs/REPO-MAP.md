# Repository map

What every top-level folder holds, and where to look for what.

```
HorizonX/
  programs/      Every compiled program, ready to run (HorizonX.exe, the engine, the proxy, plugins, Java layer)
  host/          HorizonX, the desktop app (setup wizard, dashboard, server pages)
  app/           horizonx-core: the app's engine room (servers, downloads, plugins, playit, updates)
  horizonsx/     HorizonsX, the server engine (a fork of Horizon)
  sdk/           Everything for writing plugins
  java-layer/    Runs Bukkit/Spigot/Paper .jar plugins unchanged
  packaging/     Icons and platform packaging
  vendor/horizon Upstream Horizon (git submodule)
  docs/          This documentation
```

## `host/` and `app/`: HorizonX

| Path | What |
|---|---|
| `host/src/` | The window: home, setup wizard (server type, plugins, world, performance, review), the dashboard and each server's tabs (Overview, Console, Players, Plugins, Performance, Backups, Schedule, Settings, Files) |
| `app/src/` | The library the window drives: making and starting servers, the engine download, the plugin catalogue, playit.gg addresses, schedules, backups, self-update. Also builds `horizonx-cli`, the same features from a terminal |
| `app/docs/`, `app/README.md` | Notes on the core library |

Build: `cargo build --release` in `host/`.

## `horizonsx/`: the server engine

A fork of [Horizon](https://github.com/horizon-rust/horizon), Minecraft: Java Edition 26.3 rewritten in Rust, tuned
for many players (region threads, multi-core sending, hot-reloaded plugins).

| Path | What |
|---|---|
| `crates/horizon-server/`, `horizon-world/`, `horizon-protocol/`... | The engine itself |
| `crates/horizon-client*/` | The Horizon client (the same codebase plays and serves) |
| `plugin-layer/` | The Paper API wired into the engine: `bundle/` (the bridge plugins talk to), `family/` (the hook patches that fire events) |
| `plugins/` | 22 Rust plugins built for HorizonsX (EssentialsX in `essentialsx/`, LuckPerms, mcMMO, Multiverse, DeluxeMenus, TAB, Plan, jobs, auctions, vaults...), each a crate, with their own copy of `horizon-kit` |
| `docs/v2/README.md` | Every HorizonsX change, how it differs from vanilla, and its switch |
| `README.md` | Upstream Horizon's own README |
| `tools/`, `xtask/`, `bench/`, `benchmarks/`, `tests/` | Engine tooling, benchmarks and tests |

## `sdk/`: writing plugins

| Path | What |
|---|---|
| `horizon-bukkit/` | The Bukkit/Paper API in Rust. [`AI_GUIDE.md`](../sdk/horizon-bukkit/AI_GUIDE.md) is the quick guide; [`API_INDEX.md`](../sdk/horizon-bukkit/API_INDEX.md) maps every Java method to its Rust function |
| `horizon-kit/` | The helper library most plugins use: MiniMessage, menus, items, configs, stores, sounds, timers. [`KIT.md`](../sdk/horizon-kit/KIT.md) |
| `horizon-mod/` | The lower-level interface `horizon-bukkit` sits on |
| `examples/` | Tested plugins to copy: `heal`, `joinmessage`, `autobroadcast`, `blockstats`, `menu` |
| `example-welcome/` | A minimal `horizon-mod` mod |
| `cookbook/` | Compiled recipes for common jobs; rendered in [COOKBOOK.md](COOKBOOK.md) |
| `mcp/` | The `plugin-port` MCP server for AI assistants: [MCP-SETUP.md](MCP-SETUP.md), [MCP-TOOLS.md](MCP-TOOLS.md) |

## `java-layer/`: unchanged Java plugins

| Path | What |
|---|---|
| `rust/` | The server plugin that starts a JVM only when `plugins/` holds a `.jar` |
| `java/` | The Bukkit/Paper classes those jars call, backed by the Rust server |
| `runtime/` | The bundled Java runtime setup |
| `linux/` | Linux build scripts |
| `README.md`, `COVERAGE.md` | How it works, and which API is covered |

## `docs/`

| Page | For |
|---|---|
| [README.md](README.md) | Where to start |
| [GETTING-STARTED.md](GETTING-STARTED.md) | Your first plugin |
| [PLUGIN-GUIDE.md](PLUGIN-GUIDE.md) | How to do everything plugins do |
| [COOKBOOK.md](COOKBOOK.md) | Working code |
| [MCP-SETUP.md](MCP-SETUP.md), [MCP-TOOLS.md](MCP-TOOLS.md) | The AI tools |
| [TESTING.md](TESTING.md) | Testing on a private server |
| [DESIGN.md](DESIGN.md) | HorizonX's design and feature map |
| [launcher-commands.md](launcher-commands.md) | The command-line interface |
