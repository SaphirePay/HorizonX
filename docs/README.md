# Documentation

Everything in this repository, and how to use it. Start with the page for what you want to do.

| I want to... | Read |
|---|---|
| Run a server with the app | The main [README](../README.md#download) |
| Write my first plugin | [GETTING-STARTED.md](GETTING-STARTED.md) |
| Look up how to do something in a plugin (items, events, menus, blocks, configs...) | [PLUGIN-GUIDE.md](PLUGIN-GUIDE.md) |
| Copy working code for a common job | [COOKBOOK.md](COOKBOOK.md) |
| Let an AI assistant write plugins with me | [MCP-SETUP.md](MCP-SETUP.md), then [MCP-TOOLS.md](MCP-TOOLS.md) |
| Test plugins on a private server, with screenshots of items in game | [TESTING.md](TESTING.md) |
| Know what each folder in this repository is | [REPO-MAP.md](REPO-MAP.md) |
| Run Paper/Spigot `.jar` plugins unchanged | [`java-layer/README.md`](../java-layer/README.md) |
| Understand the engine and what differs from vanilla | [`horizonsx/docs/v2/README.md`](../horizonsx/docs/v2/README.md) |
| Look up every Bukkit/Paper method the SDK has | [`sdk/horizon-bukkit/API_INDEX.md`](../sdk/horizon-bukkit/API_INDEX.md) |
| Read the SDK's own quick guide | [`sdk/horizon-bukkit/AI_GUIDE.md`](../sdk/horizon-bukkit/AI_GUIDE.md) |
| Use the helper library most plugins are built on | [`sdk/horizon-kit/KIT.md`](../sdk/horizon-kit/KIT.md) |

## The short version

- A plugin is a Rust library built on `horizon-bukkit`, the Bukkit/Paper API with the same classes and the Java names
  in `snake_case`. If you can write it for Paper, you can write it here.
- `horizon-kit` sits on top: MiniMessage text, chest menus, item builders, configs, sounds, stores, timers.
- Build it with `cargo build --release` and drop the `.dll` (Windows) or `.so` (Linux) into a HorizonsX server's
  `plugins/` folder. The server swaps it in while it runs.
- The `plugin-port` MCP server answers "can a plugin do X, and how?" in about a second, so neither you nor an AI has
  to read the SDK's 11,000 functions by hand.
