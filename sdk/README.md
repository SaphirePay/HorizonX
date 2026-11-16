# Horizon SDK

Write server plugins for HorizonsX in Rust, with the Bukkit/Paper API.

| Folder | What |
|---|---|
| [horizon-bukkit](horizon-bukkit) | The Bukkit/Paper API in Rust: the same classes, Java names in `snake_case`. Start with [AI_GUIDE.md](horizon-bukkit/AI_GUIDE.md); every method is in [API_INDEX.md](horizon-bukkit/API_INDEX.md) |
| [horizon-mod](horizon-mod) | The lower-level mod interface `horizon-bukkit` is built on |
| [examples](examples) | Tested plugins: `heal`, `joinmessage`, `autobroadcast`, `blockstats`, `menu` |
| [example-welcome](example-welcome) | A minimal `horizon-mod` mod |
| [horizon-kit](horizon-kit) | Helpers most plugins use: MiniMessage, menus, items, configs, stores, sounds, timers ([KIT.md](horizon-kit/KIT.md)) |
| [cookbook](cookbook) | Compiled recipes for common jobs ([docs/COOKBOOK.md](../docs/COOKBOOK.md)) |
| [mcp](mcp) | The `plugin-port` MCP server for AI assistants ([docs/MCP-SETUP.md](../docs/MCP-SETUP.md)) |

## Build a plugin

```bash
cd examples/heal
cargo build --release
```

Copy `target/release/heal.dll` (Windows) or `libheal.so` (Linux) into a server's `plugins/` folder. HorizonsX loads it
while the server runs.

More: [docs/](../docs/README.md).

Licensed under Apache-2.0.
