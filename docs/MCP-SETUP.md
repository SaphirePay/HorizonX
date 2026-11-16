# The plugin MCP server: setup

[`sdk/mcp/`](../sdk/mcp) is an MCP server (`plugin-port`) that gives an AI assistant 93 tools for Horizon plugins:
search the whole SDK in a second, get compiled example code, see which events the engine really fires, port Java
plugins, build, hot-drop and screenshot plugins on a test server. The full list is in [MCP-TOOLS.md](MCP-TOOLS.md).

## Why it exists

The SDK has over 11,500 functions and 308 events. Reading it to answer "can a plugin do X?" takes minutes and
misses things: a function in an unexpected module, an event that compiles but never fires, a helper someone already
wrote. The tools answer those questions directly:

| Question | Tool | Example |
|---|---|---|
| Can a plugin do X, and with what? | `sdk_can` | `sdk_can "hunger"` -> `FoodLevelChangeEvent`, `EntityExhaustionEvent` (both fire), `Player::set_food_level`... |
| Show me working code | `sdk_recipe` | `sdk_recipe "right click item"` -> the compiled recipe and its imports |
| What exactly is this function, and how do I import it? | `sdk_doc` | `sdk_doc action_is_right_click` -> `horizon_bukkit::core2::action_is_right_click` + source |
| How do real plugins call it? | `sdk_usage` | `sdk_usage send_action_bar` -> the call in context, one example per plugin |
| Does the engine fire this event? | `sdk_fires` | `sdk_fires EntityExhaustionEvent` -> where, plus events that share its hook |
| What is in this part of the SDK? | `sdk_map` | `sdk_map Player`, `sdk_map entity`, `sdk_map kit:gui` |

## Install

You need Python 3.11 or newer. No packages are required for the SDK tools.

With Claude Code, from the repository root:

```bash
claude mcp add --scope user plugin-port -- python -I sdk/mcp/plugin_mcp.py
```

Use an absolute path if you register it from elsewhere. Any MCP client works the same way: the server speaks MCP over
stdin/stdout. Restart or reconnect the client (`/mcp` in Claude Code) to see the tools.

## What works where

| Tools | Need | Notes |
|---|---|---|
| `sdk_can`, `sdk_map`, `sdk_doc`, `sdk_recipe`, `sdk_fires`, `sdk_usage`, `sdk_search`, `kit_doc` | This repository only | They read `sdk/`, `sdk/cookbook/` and `horizonsx/plugin-layer/`. The first call after a change takes a few seconds, then answers are cached |
| `rig_*`, `check_*`, `server_*`, `plugin_parity` | A workspace with a test server and a Horizon client | See [TESTING.md](TESTING.md) |
| `plugin_port`, `plugin_convert`, `plugin_rewrite`, `jl_*` | A workspace with the plugin corpus, Java 25, and Vineflower for decompiling | For porting Java plugins at scale |
| `uui_*` | A workspace with the UI pack tools | Resource-pack menus |

## Settings

| Variable | Default | What |
|---|---|---|
| `HORIZON_WORKSPACE` | `~/Horizon` | The folder holding engine checkouts, `plugin-ports/` (your plugins), the plugin corpus and test servers. The SDK tools use this repository first and the workspace second |
| `HORIZON_JAVA` | `java` on the PATH | The Java the porting and Java-layer tools run |
| `HORIZON_TMP` | `<workspace>/tmp` | Where `rig_drop` stages a build before moving it into place |

`sdk_usage` searches hand-written plugins: `sdk/examples/`, `horizon-kit`, and crates in
`<workspace>/plugin-ports/` whose names end in `_rs` (machine-converted crates are skipped: they are large and make
poor examples).

## Keeping it accurate

- The SDK tools read the source every time it changes, so they never go stale.
- [MCP-TOOLS.md](MCP-TOOLS.md) and [COOKBOOK.md](COOKBOOK.md) are generated from the server and the cookbook.
- Recipes are a real crate: run `cargo check` in `sdk/cookbook` after an SDK change, and a broken recipe fails the
  build.
