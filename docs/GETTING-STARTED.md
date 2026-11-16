# Getting started: your first plugin

This page takes you from nothing to a plugin running on a server. It takes about ten minutes.

## 1. What you need

| Tool | Why | Get it |
|---|---|---|
| Rust (stable, 1.99 or newer) | Builds plugins | <https://rustup.rs> |
| A HorizonsX server | Runs them | Horizon Host makes one in five steps ([README](../README.md#download)), or build `horizonsx/` yourself |
| Python 3.11+ (optional) | Runs the MCP tools for AI assistants | <https://python.org> |

On Windows, Rust needs the Visual Studio C++ build tools (the Rust installer offers them).

## 2. Copy an example

The fastest start is a tested example. From the repository root:

```bash
cp -r sdk/examples/heal myplugin
cd myplugin
```

A plugin folder holds three files:

```
myplugin/
  Cargo.toml    the crate: name, cdylib, the SDK as a path dependency
  plugin.yml    exactly as on Paper: name, version, main, commands, permissions
  src/lib.rs    the code
```

Edit `Cargo.toml` so the paths point at this repository's SDK. If your folder sits beside `sdk/`:

```toml
[package]
name = "myplugin"
version = "1.0.0"
edition = "2024"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
horizon-bukkit = { path = "../sdk/horizon-bukkit" }
horizon-kit = { path = "../sdk/horizon-kit" }   # optional, but most plugins want it
serde_json = "1"

[workspace]
```

Change `name:` in `plugin.yml` to your plugin's name. The `main:` line can be any text; it is kept for Paper
compatibility.

## 3. Write the code

A whole plugin with a command and an event:

```rust
use horizon_bukkit::event::PlayerJoinEvent;
use horizon_bukkit::{CommandSender, Plugin, bukkit_plugin, enums};
use horizon_kit::app;

fn on_enable(plugin: Plugin) {
    app::set(plugin); // lets app::info, app::folder and app::later work anywhere
    plugin.set_executor("hello", |sender: &CommandSender, _label, _args| {
        sender.send_message(&horizon_kit::mini::legacy("<#a855f7>Hello <white>from Rust!"));
        true
    });
    plugin.register_event_with(enums::event_priority::MONITOR, false, |e: &mut PlayerJoinEvent| {
        app::info(&format!("{} joined", e.get_player().get_name()));
    });
}

fn on_disable(_plugin: Plugin) {
    app::clear();
}

bukkit_plugin!(on_enable, on_disable);
```

Add the command to `plugin.yml`:

```yaml
commands:
  hello:
    description: Say hello
```

## 4. Build and install

```bash
cargo build --release
```

The plugin is `target/release/myplugin.dll` on Windows, or `target/release/libmyplugin.so` on Linux. Build on the
system the server runs on, or build the `.so` in a Linux container (`docker run --rm -v "$PWD:/src" -w /src
rust:1-bookworm cargo build --release`).

Copy the file into the server's `plugins/` folder. **There is no restart.** HorizonsX notices the new file and swaps
the plugin in while the server runs:

- A server with players on swaps within a second; the log says `[HotReload] swapped 1 ...`.
- An empty server is paused and swaps when the next player joins.
- To update a plugin, copy the new build over the old one. The server reloads a file when its modified time changes,
  so copy it with a fresh time (copy, then move it into place) rather than restoring an old timestamp.

## 5. Next steps

- How to do anything in a plugin: [PLUGIN-GUIDE.md](PLUGIN-GUIDE.md).
- Working code for common jobs: [COOKBOOK.md](COOKBOOK.md).
- Look up a Java method you know: `grep "Player.setHealth" sdk/horizon-bukkit/API_INDEX.md`.
- Let an AI assistant do the looking up: [MCP-SETUP.md](MCP-SETUP.md).
