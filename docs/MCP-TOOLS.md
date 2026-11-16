# MCP tools reference

The `plugin-port` MCP server in [`sdk/mcp/`](../sdk/mcp) gives an AI assistant (Claude Code, or any MCP client)
93 tools for writing, porting and testing Horizon plugins. This page lists every one of them; it is generated
from the server itself, so it matches the code.

Setup, environment variables and what works without a workspace: [MCP-SETUP.md](MCP-SETUP.md).

## Contents

- [Find out what the SDK can do](#find-out-what-the-sdk-can-do) (25)
- [Port or convert a Java plugin](#port-or-convert-a-java-plugin) (19)
- [Test on a private server (the rig)](#test-on-a-private-server-the-rig) (12)
- [Java layer (unchanged plugin jars)](#java-layer-unchanged-plugin-jars) (8)
- [Mods](#mods) (8)
- [Menus drawn by a resource pack (UUI)](#menus-drawn-by-a-resource-pack-uui) (18)
- [Video](#video) (3)

## Find out what the SDK can do

Start here before reading SDK source. Every answer comes back in about a second.

| Tool | Arguments | What it does |
|---|---|---|
| `sdk_can` | `query`, `limit` | Can a plugin do X? One search over every horizon-bukkit function, every event (with whether the engine really fires it) and the horizon-kit helpers, best match first. Use before reading SDK source: `hunger`, `enderman target`, `crop age`, `action bar`. |
| `sdk_map` | `area` | The whole SDK at a glance (types and method counts, events by family and how many fire, kit modules, events that never fire). With an area: a type (Player, ItemStack, World...) = all its methods; an event family (entity, player, block...) = its events and whether each fires; kit:<module> = its helpers. |
| `sdk_recipe` | `task` | Working, compiler-checked code for a common plugin job (skeleton, command, chat, action bar, sound, particles, custom item with pack model/glint/tag, item tags, inventory slots, right-click item, timers, config, menu, blocks near, crop age, extra drops, hunger, mob target, fake blocks). No task = the list. |
| `sdk_doc` | `name` | One SDK item in full: the `use` path, doc, Java name, whole definition, what is not exact, and how many of our plugins use it. |
| `sdk_usage` | `name`, `limit` | Real uses of an SDK or kit name in our own plugins, hand-written ones first, with the code around each: the fastest way to see how a call is really used. |
| `sdk_fires` | `event` | Where the engine fires an event (name, Java class or hook id): every site with its function and doc, the code around the first, and other events sharing the same hook. Says plainly when nothing fires it. |
| `sdk_search` | `query`, `limit` | Find Rust in horizon-bukkit by words, with signatures, file and line, and any note on what is not exact. |
| `sdk_refresh` | - | Reread the SDK and rerate every plugin. |
| `kit_doc` | `topic` | horizon-kit reference without reading source: no topic = module list + KIT.md recipe; a module (gui, item, store, hooks, mini, yaml, wire, sync, time, sound, hocon, packets, cmd, config...) = its doc and public signatures. |
| `api_lookup` | `java` | The horizon-bukkit Rust for a Java class or method. |
| `api_kit` | `java` | Everything to implement one Paper method: signature, Javadoc, demand, real uses, likely engine code. |
| `api_gaps` | `limit` | What to add to horizon-bukkit next, by how many plugins want it. |
| `event_info` | `query`, `limit` | A Paper event: how to listen, what it reads and changes, what is not exact, and if the last probe saw it fire. Empty for the whole picture. |
| `event_results` | `name` | What the last event comparison found: events firing on Paper only, on Horizon only, and counts far apart. Run one with check_start kind=events: every event class listened to on real Paper and on Horizon, the same real actions played on both, a minute and a half with the rig up. |
| `hook_search` | `query`, `limit` | The engine hooks that exist: ids, targets, what they pass. |
| `hook_seam` | `function`, `file` | Which injection kinds an engine function takes, before writing a hook. |
| `used_by` | `java`, `limit` | Real plugin code that calls a Paper method. |
| `search_horizon` | `pattern`, `limit` | Regex search of Horizon's engine source. |
| `search_plugins` | `pattern`, `limit` | Regex search of every decompiled plugin. |
| `method_body` | `id`, `name`, `cls`, `limit` | A method's code by name in a plugin's own Java (decoded, libraries skipped), any overload, optionally in one class (`cls`), with the class's extends chain. `id`: a plugin-ports id or a folder. |
| `trace_key` | `id`, `text`, `exact`, `limit` | Every place a string is used in a plugin (a YAML key, permission, message, packet name): file, line, the method around it, and who calls that method. exact=true for the whole literal only. |
| `config_schema` | `id`, `limit` | Every config key a plugin reads (getString/getInt/... and contains), with type, default and where; then dotted strings it reads through its own helpers. |
| `packet_map` | `id` | The packets a plugin reads and sends (PacketEvents, ProtocolLib). For each read: the Bukkit event with the same input on Horizon and whether the staged engine fires it. |
| `client_caps` | `query` | What the Horizon client supports that a plugin relies on (pack text shaders on GUI and world text, SetCamera, passengers, displays, interaction boxes, boss bars, packs, fonts, dialogs), with where in the client source. |
| `maker_guide` | `topic` | How to make a plugin, a mod or an engine hook here, and the traps. Topics: plugin, mod, hook, test, traps. |

## Port or convert a Java plugin

From a Bukkit/Spigot/Paper jar (or its source) to a Rust plugin.

| Tool | Arguments | What it does |
|---|---|---|
| `plugin_port` | `source`, `server`, `version`, `id`, `force` | THE ONE TO USE when someone asks to convert, port or install a Java plugin (Bukkit, Spigot, Paper) for Horizon. One call does it all: finds the jar (a Modrinth name like "LuckPerms", a download link, or a path), converts it to Rust, builds it, starts it once on a scratch server to see it enable, and with `server` (a launcher server name like "test") installs it there, backing up and restarting the server; it refuses while players are on unless force. Takes minutes for a big plugin. The reply gives each step and an honest note of what may not work. |
| `plugin_port_many` | `sources`, `server`, `force`, `workers` | Like plugin_port for any number of plugins at once: `sources` is a comma-separated list of Modrinth names, links or paths. They convert side by side (`workers` at a time, 3 by default), then every one that enabled cleanly goes onto `server` together with one backup and one restart. |
| `plugin_convert` | `jar`, `id` | Convert a plugin jar to Rust automatically, and build it: seconds, not hours. Gives what share of its methods run and what is missing. Takes a path or part of a jar name. |
| `plugin_rewrite` | `source`, `id` | THE START OF A NATIVE REWRITE ("rewrite X in Rust"): a jar path, corpus name, source folder/zip, or a plugin to fetch (name, github:owner/repo, modrinth:slug, hangar:owner/slug, spiget:id, URL) to a horizon-kit crate that builds and loads in ~30 s; BRIEF.md has an SDK map of every Bukkit call it makes: config + messages, a router with every sub-command found (permissions, usage), events/timers noted, BRIEF.md and tests/check.py. See plugin-tools/kit/KIT.md. |
| `rewrite_build` | `crate`, `release` | Build a native rewrite crate (plugin-ports/<name> or a path) with the shared target dir; a few lines back: ok, the DLL, `file:line: error` with help lines, warning count. |
| `plugin_brief` | `name` | Everything needed to port a plugin: decompiled source, events, and the Rust for each Java call. |
| `plugin_queue` | `limit`, `tier` | The next plugins to port, easiest first. |
| `plugin_scaffold` | `name` | Start a port: crate, plugin.yml, resources, stubs and a parity script. |
| `plugin_build` | `id` | Build a port into its DLL. |
| `plugin_parity` | `id` | Prove a port 1:1: same script on the Java plugin on Paper and the port on Horizon. |
| `plugin_import` | `source` | Bring in any Bukkit, Spigot or Paper jar by path or URL: decompile it, count it, and say what a port takes. plugin_brief and plugin_scaffold then take it. |
| `plugin_new` | `name`, `commands`, `description` | Start a new plugin from nothing: a crate that builds and loads as it stands. |
| `plugin_try` | `id`, `commands`, `seconds`, `screenshot`, `force`, `new_engine`, `survival` | One call from DLL to chat: deploy, join with a test client, run commands, return chat, trouble and a screenshot. Commands are split on ; or newlines. new_engine puts the newest engine build on first; the client plays in creative unless survival is set. |
| `plugin_check` | `id` | Compile a plugin or mod without building it, suggesting close SDK names for what does not exist. |
| `plugin_audit` | `id` | What keeps a plugin or mod from being finished: stubs, commands with no code, SDK calls that are not exact. |
| `plugin_deploy` | `id`, `force` | Put a plugin or mod DLL on the test server and restart it. Refuses while someone plays unless forced. |
| `plugin_remove` | `id`, `force` | Take a plugin or mod DLL off the test server and restart it. |
| `java_decode` | `id` | Decode an obfuscated plugin's Java (AES-encrypted strings, rotate/xor numbers, fake throw branches) into java_clean/ beside it. plugin_import and new_rewrite.py already do it; this is for Java decompiled before. |
| `java_hints` | `file` | A Java file against the SDK: each listener and API call it has, the Rust for it, and what has none. |

## Test on a private server (the rig)

Build, hot-drop, look at items in game, and check a server. The rig tools need the workspace test server (see TESTING.md).

| Tool | Arguments | What it does |
|---|---|---|
| `rig_drop` | `crate`, `release` | Build a plugin crate (plugin-ports/<crate>) and hot-drop its DLL onto the private test server (the jobs rig :25797) with a fresh file time. Errors come back as file:line. |
| `rig_shots` | `commands`, `item`, `name`, `crop`, `close`, `fill_hotbar` | See items in game: on the rig, as the muted offscreen Tester (op), clear the inventory (hotbar filled so gives land where tooltips fit), run `;`-separated commands, open the inventory, hover each slot with `item` (e.g. clock) and screenshot every tooltip. Gives image paths cropped to each tooltip; closes the client after. |
| `rig_rejoin` | - | Reconnect the rig Tester client and wait for server packs to load (after any server pack change: a change during a join can leave the client on old packs). |
| `rig_doctor` | - | Why a test server would not start or would refuse a DLL, before a test finds out: prepared game files, engine build age, who is on. |
| `check_start` | `kind` | Start a check in the background: fast (conformance and the event comparison, about 4 minutes), full (those with parity and the proof, about 8 minutes), events, conformance, probe, parity or proof. |
| `check_status` | `lines` | How the check from check_start stands. |
| `server_status` | - | The test server: up or not, who is on, what it loads, and trouble in its log. |
| `server_log` | `filter`, `lines` | The test server's log, filtered by a regular expression. |
| `engine_check` | - | Regenerate the bundle and type-check the engine with every mixin in, giving errors and planner refusals. |
| `thread_stacks` | `match`, `everything` | Where the threads of a running engine are, for a frozen or slow server: no debugger needed. |
| `lock_scan` | - | Engine code holding a static lock across a call that could want it again. |

## Java layer (unchanged plugin jars)

Run real Bukkit/Paper jars on HorizonsX and see what they need.

| Tool | Arguments | What it does |
|---|---|---|
| `jl_build` | `java`, `rust`, `gen` | Build the Java layer (Java side -> java/out/lib; Rust DLL; gen=true also regenerates the SDK binding table first). Short OK/FAILED with the errors. |
| `jl_log` | `lines` | The live Java layer server's last layer/error log lines. |
| `jl_probe` | `snippet`, `probes`, `accept` | Java layer checks in a scratch server: snippet = Java statements (body of onEnable; t("name", () -> value) records a check); or no snippet = the whole probe suite (test/probes, probes = some names) compared with its baseline (accept=true makes this run the baseline). |
| `jl_report` | `run`, `vs` | A Java layer sweep: progress, score (works / enabled, vs Paper), failures grouped by cause, top layer gaps and missing classes; vs = an older run for gained/lost. |
| `jl_scan` | `jar`, `top` | Static check of a plugin jar (path or corpus name) for the Java layer: plugin.yml, Java version, NMS/CraftBukkit links and reflection, PacketEvents, Netty, other plugins, unresolved libraries. No jar / top=true: what blocks the most corpus jars. |
| `jl_sweep` | `run`, `only`, `group`, `lanes`, `failed`, `paper_ok`, `settle` | Run corpus plugins on the Java layer + HorizonsX engine in the background (groups, crash-splitting). only = comma names; failed = an older run (only its failures); paper_ok = only jars known to enable on real Paper. Check with jl_report. |
| `jl_try` | `jars`, `commands`, `restart` | The live Java layer test server (<workspace>/parity/jl_live, a client on it as op Prover): jars = comma paths or corpus names (restarts with them + their deps), commands = `;`-separated without slash; gives chat replies and layer log lines per command. |
| `jl_why` | `run`, `plugin` | One plugin's row in a sweep: state, errors with stack frames, gaps, missing classes, static facts. |

## Mods

Fabric-style mods on the Horizon client and engine.

| Tool | Arguments | What it does |
|---|---|---|
| `fabric_event` | `query`, `limit` | A Fabric API event: its status here, and where its Java is. |
| `mod_build` | `id` | Build a mod's DLL and pack its .hmod (manifest, assets, data, DLL): seconds, no game build. |
| `mod_events` | `query`, `limit` | What host.on can hear: each event, its payload, and if a refusal works. |
| `mod_install` | `id`, `instance`, `on` | Put a mod's .hmod into a launcher instance (default: the Mods instance) and switch it on. |
| `mod_list` | - | The mods in mod-ports and whether each has a built .hmod. |
| `mod_new` | `name`, `description`, `commands`, `key`, `side` | Start a new mod (or a rewrite of one): a crate that builds into an .hmod with its code inside and runs in single player as it stands (key, HUD line, join message). Write src/lib.rs with horizon-kit (kit_doc client: HUD, keys, crosshair, outlines) and horizon-bukkit events. |
| `mod_scan` | `source` | What a Fabric mod jar needs, by path or URL: events ported or not, and its Mixin count. |
| `mod_try` | `id`, `steps`, `screenshot`, `keep`, `seconds` | Play a mod in a real single-player game (offscreen): the .hmod on, a fresh world, then the steps (horizon-ctl lines split by ;, `line => expected`, `wait 2`, `screenshot`, {x} {y-1} {z} = player position). Gives replies, mod log, trouble, screenshots. |

## Menus drawn by a resource pack (UUI)

Pixel-placed menus: pages, elements, images, keyframes, screenshots.

| Tool | Arguments | What it does |
|---|---|---|
| `uui_action_add` | `server`, `page`, `target`, `action`, `reload` | Add click actions (one or a list, YAML/JSON) to an element; uui_reference actions lists them. |
| `uui_actions_set` | `server`, `page`, `target`, `actions`, `reload` | Replace an element's actions with a list. |
| `uui_check` | `server`, `page` | A page's layout as the plugin computes it (every element's box) and its problems (missing images, off-page elements), without a server. |
| `uui_command` | `server`, `line` | Run a /uui command on a running server from the console (open <page> <player>, close <player>, list, reload, world ..., pack <player>). |
| `uui_element_add` | `server`, `page`, `element`, `parent`, `index`, `reload` | Add an element or a list of them (YAML/JSON) to a page, at the top level or under `parent` (id or path). Ids are made when missing. |
| `uui_element_edit` | `server`, `page`, `target`, `changes`, `reload` | Change an element's properties: `changes` maps dotted keys to values (YAML/JSON), null deletes (e.g. {"position.x": 200, "size.width": 400, "color": "2563eb", "text": "Buy"}). |
| `uui_element_move` | `server`, `page`, `target`, `parent`, `index`, `reload` | Move an element under another parent (or the top level) at an index. |
| `uui_element_remove` | `server`, `page`, `target`, `reload` | Remove an element and its children. |
| `uui_image_add` | `server`, `source`, `name` | Add an image (file path or URL) to contents/images and reload so the pack carries it. |
| `uui_keyframe` | `server`, `page`, `target`, `track`, `tick`, `value`, `reload` | Set (or with null remove) an open-animation keyframe on an element: track position\|scale\|rotation\|opacity, tick, value. |
| `uui_page_delete` | `server`, `page`, `reload` | Delete a page (moved to contents/backups). |
| `uui_page_get` | `server`, `page` | A page's YAML, block tree (paths and ids) and each element's laid-out box from the plugin's own layout. |
| `uui_page_new` | `server`, `page`, `width`, `height`, `command`, `backdrop`, `reload` | Create an empty page (size, optional /command, dimmed backdrop), then reload. |
| `uui_page_write` | `server`, `page`, `content`, `reload` | Create or replace a whole page from YAML text (old copy kept in contents/backups), check its layout, reload. |
| `uui_pages` | `server` | A server's UltimateUI pages, images and components (server: launcher server name, folder, or empty for the test rig). |
| `uui_reference` | `topic` | UltimateUI page format: how to write pages, elements, actions, animations, world UIs and images (topic: page, elements, actions, animation, world, images; empty = all). Read before making a UI. |
| `uui_screenshot` | `page`, `server`, `seconds`, `look` | See a page: it is copied to the test rig, opened for the test client and screenshotted (the newest UltimateUI build). Refuses while someone plays on the rig. |
| `uui_world_place` | `server`, `id`, `page`, `world`, `x`, `y`, `z`, `yaw`, `size`, `look_at`, `interactive`, `per_player`, `reload` | Place a page in the world as a world UI (id, page, world, x, y, z, yaw, size in blocks, look_at off\|vertical\|on, interactive, per_player). |

## Video

Record and edit gameplay from a Horizon client.

| Tool | Arguments | What it does |
|---|---|---|
| `video_edit` | `command` | Edit a video. A line of tools/video.py: info IN, trim IN OUT --from S --to S, join OUT IN IN [--fade S], speed IN OUT FACTOR, title IN OUT TEXT [--at S --for S], sheet IN OUT [--stills N], still IN OUT --at S, compress IN OUT [--crf N]. |
| `video_record` | `path`, `fps`, `home` | Start recording video and sound in a running Horizon client, windowed or --offscreen (not --headless). Ends with video_stop. |
| `video_stop` | `home` | Finish the recording and join its sound. Gives the path, frames, seconds and whether it has sound. |

