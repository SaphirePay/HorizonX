# Testing plugins on a private server

Test every plugin change on a private server before it goes to players. The `rig_*` MCP tools automate the loop:
build, drop onto the test server, run commands as a test player, and screenshot what players will see.

## The rig

The tools expect this layout in your workspace (`HORIZON_WORKSPACE`, default `~/Horizon`):

| Path | What |
|---|---|
| `parity/jobs/server/` | A HorizonsX test server on port 25797, kept running by a supervisor |
| `parity/pwarps/pw.py` | A small driver for an offscreen Horizon client named `Tester`, with sound off |
| `parity/jobs/packsrv/` | The test resource pack, served over HTTP on port 8174 |

`Tester` is an operator on the test server, so tests can run any command without permission plugins.

## The loop

| Step | Tool | What it does |
|---|---|---|
| Build and install | `rig_drop myplugin_rs` | `cargo build --release` in `<workspace>/plugin-ports/myplugin_rs`, then copies the DLL in with a fresh time so the server swaps it in. Build errors come back as `file:line` |
| See items in game | `rig_shots "give Tester diamond; hitems give Tester relic" clock relic` | Clears the inventory, fills the hotbar so new items land where tooltips fit, runs the commands, hovers every matching item and saves one screenshot per tooltip, cropped to the tooltip |
| After changing the pack | `rig_rejoin` | Reconnects and waits for the server packs to load |

The client is closed after each call (and any copy left running is stopped by its process id), so nothing keeps
playing sound or using memory in the background.

## Good habits

- **Test where it's allowed.** A protected spawn world undoes block breaks; use a world without protection for
  block, farming and drop tests.
- **Test the real path.** If players right-click an item, have the test client right-click it (`use-item`), not a
  command that fakes the result.
- **Have a control.** To prove a power works, run the same test with it off. Watch for a power that seems to work
  while switched off: that means it leaks from somewhere else.
- **Prove it in the log.** A one-time log line the first time a feature really runs (`"leaf_shaker works: ..."`) is
  proof that costs nothing on a live server.
- **Watch the tick.** Check the server's tick time while your feature runs with several players.
