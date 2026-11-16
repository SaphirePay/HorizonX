Horizon plugin rewrites: the fast way
=====================================

A rewrite (a Java plugin written again in Rust, native, faster than the Java) starts from the plugin, not a
blank file, and is written with horizon-kit so only the plugin's own logic is new work.

1. Start (about 30 s)
---------------------

    MCP:  plugin_rewrite <source>
    CLI:  python D:\Horizon\plugin-tools\kit\new_rewrite.py <source> D:\Horizon\plugin-ports\<name>_rs

`<source>`: a jar path; a name in the corpus; a source folder or GitHub zip; or `--fetch NAME` (CLI) / just
the name (MCP), which finds it on Modrinth > Hangar > Spiget > GitHub by EXACT name. Ambiguous names: give
`github:owner/repo`, `modrinth:slug`, `hangar:owner/slug`, `spiget:id` or a URL (a miss prints the closest).

It writes a crate that builds and loads as it stands:

  - plugin.yml (commands a framework registers in code added), Cargo.toml (semver, rlib for tests)
  - data/: the configs and the DEFAULT language only (others listed); a config.yml rebuilt from the code's
    `addDefault`/`getX("k", default)` calls when the jar ships none
  - java/: decompiled source (or the real source, Java or Kotlin, for source input)
  - BRIEF.md: commands/subs/permissions, events (SDK gaps marked), timers, config defaults, biggest classes,
    and the **SDK map**: every Bukkit/Paper/Adventure call the plugin's code makes with its Rust path and
    signature, most used first, plus **kit first** hints where the kit does it better, and the imported classes
    the SDK lacks. Read BRIEF.md instead of grepping API_INDEX.
  - src/lib.rs: a stub (Router per command, a Sub per sub-command, reload/help). Replace it whole: `rm` it first,
    then write yours (the editor won't overwrite a file it hasn't read).
  - tests/check.py: each command on the live test rig

2. Write (most of the time)
---------------------------

`use horizon_kit::prelude::*;` brings the SDK prelude, the kit modules and the usual helpers.
`kit_doc <module>` (MCP) prints a module's doc and signatures without reading source.

| Need | Kit |
|---|---|
| plugin handle, folder, log, run later | `app::{set, get, folder, info, warn, later}` |
| config.yml / messages with defaults | `Config::load(folder, "config.yml", include_str!(..))`, `Lang` (`{prefix}`, `%s`, `{0}`) |
| any YAML node, Bukkit's way | `yaml::{string, list, list_or_one, int, num, boolean, is_list, is_section, keys, contains}` |
| `.conf` (Configurate/HOCON) | `hocon::read(path) -> serde_json::Value`, `hocon::write` |
| commands + subs + tab | `Router::new("cmd").sub(Sub::new("x", f).perm(..).usage(..))`; `fn f(c: &Ctx) -> Res` |
| small capitals (ᴛᴇʟᴇᴘᴏʀᴛ ᴘᴀᴅ) | `smallcaps::small("Teleport Pad")`; in any message `<small>text</small>` (mini parses it; `{placeholders}`, `%papi%` and tags inside stay; `\<arg>` is text). Minecraft's font draws them, no pack |
| text | `mini::parse_mixed` (MiniMessage + `&`/hex) -> component; `mini::preprocess` (`#hex`, `<primary>`); `text::colorize` -> legacy |
| chest menus | `gui::install(plugin)` once; `gui::Menu::new(rows, title).button(slot, item, \|c\| ..).open(&p)`; `gui::Pages`; `gui::slots("9-17")` |
| yes/no, tidy rows, storage menus | `gui::confirm(&p, "Delete?", \|p\| ..)`; `gui::centered(n)` (row columns for n buttons); `Menu::on_close_items(\|p, items\| ..)` (a sell chest's contents at close) |
| inventory work | `inv::{count, take, take_all, room_for, give, give_many, plain}` (36 storage slots, one read, overflow drops at the feet) |
| who hit whom, rank perks | `hooks::attacker(&EntityDamageByEntityEvent)` (melee or the shooter of a projectile); `hooks::chat::meta_int(uuid, "myplugin.homes")` (LuckPerms meta, cached 30 s) |
| items from config | `item::Spec::from_yaml(&node).build(&fill)` (material/name/lore/model/enchants/flags/heads/glow/...) |
| items saved as text | `item::to_base64` / `from_base64` / `items_to_base64` (server thread) |
| player data | `store::Docs<T>` (JSON per uuid: `load` at pre-login, `edit`, batched off-thread saves, `save_all` on disable); `store::File<T>` |
| money, ranks, placeholders | `hooks::economy`, `hooks::chat` (prefix/suffix/meta/groups/tracks), `hooks::placeholders(text, Some(&p))`; offer yours: `hooks::provide_placeholders(&plugin, "id", ..)` |
| durations, cooldowns | `time::{parse("10m"), ticks, format, now_ms}`, `sync::Cooldowns` |
| back to the server thread | `sync::on_main(closure)` + `plugin.run_task_timer(1, 1, sync::drain)`; `sync::hold/release` keeps an empty server ticking |
| sounds | `sound::play(&p, "ENTITY_PLAYER_LEVELUP:1:1")`, `sound::key` |
| sidebars / tab numbers / boss bars | `wire::set_score` (blank/fixed number formats), `wire::boss::{add,title,progress,remove}`, sent with `packets::queue_raw` + `packets::flush` |
| client-only entities (holograms, NPCs) | `packets` + `wire` |
| per-tick work off the server thread | `worker` (no game queries on it) |
| `/x profiler` | `stats::{set_running, report, timed}` |
| a menu that looks finished | `window::menu(&p, rows, "<b>Title")` (dark window from the server pack, the game's without it; black filler) + `theme::item("DIAMOND", "Name", Lore::new().text(..).blank().field("Price", &theme::money(n)).bar(0.4, "..").click("Left-click", "to buy"))`; `theme::{back, close, next, previous, toggle, confirm, cancel, info, filler}`; `window::install(plugin)` + `window::save()` |
| forms, prompts, confirmations | `dialog::install(plugin)`; `Dialog::new("<b>Title").text(..).input_text(key, label, init, max).slider(..).toggle(..).choice(..).button("<#4ade80>Save", \|p, form\| ..).cancel("Cancel").open(&p)`; `dialog::{confirm, ask_text, ask_number, choose, notice, close}`; replaces chat prompts (real text boxes, sliders) |
| dialog layout, cards, pack | `.card(&stack, "<b>Name</b>
line")` (item + text, box fitted so the item sits beside it), `.divider()` (solid line), `.line(mini, width)`, `.slider_as(key, label, min, max, step, init, "%1$s: %2$s coins")`, `.input_text_at(.., width)`; `dialog::pack_hides_warning(true)` when the server pack blanks `gui/sprites/dialog/warning_button*` (titles get a 32 px lead-in to stay centred; zenithbackpacks_rs/pack/hide_dialog_warning.py patches a pack and darkens the in-world backdrop); `dialog::text_width(mini)` |
| menus drawn by a pack (art + live text) | `ui::Title::new().art(&arts["bank"]).text(x, y, "…", "#hex").centred(..).right(..).json()` -> `gui::Menu::raw(rows, &title)`; see "Drawn menus" below |
| invisible buttons, item pictures, animations | `gui::hotspot(name, lore)` / `gui::hotspots(menu, slots, ..)`; `item::model(&mut s, "ns:coin")`, `item::glint`; `gui::animate(&p, vec![(ticks, slot, item)])` |
| who has the pack | `pack::track(&plugin, id, url, sha1)`, `pack::ensure(id, &p, \|p\| open(p))` (sends it, opens once loaded), `pack::save/restore(folder)` |

Drawn menus
-----------

A menu's title can be a picture the size of the window (and past it: side panels up to 125 px, a banner above
windows of 4 rows or fewer) plus text at any pixel, so a menu can look like anything.

1. Draw the art with `tools/uidraw.py` (Canvas, the game's font and textures, three styles: A Vanilla+ = the game's
   own bevels in purple, B Midnight = dark cards, C Crafted = block textures with gold trim). Leave room for live
   text; `uidraw.save(c, "ui/art", "bank")` refuses art with text under an item, text over text or text out of its
   box.
2. `python tools/uipack.py ui out --namespace myplugin` -> `myplugin-pack.zip` (with the shared `kitui` fonts: text at
   window rows 5-240, pixel moves, the invisible item) + `ui.json` (each art's glyph, offsets, rows) + its sha1.
3. In the plugin: `ui::Arts::load(&folder.join("ui.json"))`; build the title with `ui::Title` (window pixels; slot
   (r, c) has its frame at `(7 + 18c, 17 + 18r)`); open with `gui::Menu::raw` (`Menu::new` runs colour codes over
   the title and would break `#rrggbb` colours in it); put `gui::hotspot`s over drawn buttons.
4. Serve the zip (`pack::serve(port, bytes)` or any web host) and `pack::track` + `pack::ensure` so whoever opens
   the menu without the pack gets it and sees the menu once it's in.

Live text: the default font (ASCII and the small capitals), widths exact (`ui::width`); rows 5 to 240 (the game
can't lift text more than a pixel above the title line, so banner text above the window must be in the art);
changing it means opening the menu again (items stay). Examples: parity/guimock (25 menus, the live profile).

3. Check
--------

    MCP:  rewrite_build <name>        (a few lines: ok, the DLL, `file:line: error` + help)
    CLI:  cargo build --release       (CARGO_TARGET_DIR=D:\Horizon\hx-plugin-build\t_rw shared by all rewrites)
    copy the DLL into the test server's plugins (hot reload, no restart), python tests/check.py

Rules learned the hard way
--------------------------

  - Every command gets a `permission:` node in plugin.yml and one parent (`myplugin.player`, default true) holding
    the player nodes, so LuckPerms can take one feature away; staff nodes under `myplugin.admin` (default op).
    Rank perks are LuckPerms meta (`lp group vip meta set myplugin.x 5`), read with `hooks::chat::meta_int`.
  - A command another plugin also has: on engines before 11-06 both ran (and EssentialsX ran the other plugin's
    again); newer engines give it to one plugin (command-owners.yml, `/commands owner <cmd> <plugin>`).
  - Git Bash turns `/word` arguments into paths: MSYS_NO_PATHCONV=1 for rig scripts that chat commands.

  - Game queries only on the server thread or inside an event: a plugin's own threads get Null back. Read on the
    server thread, hand values to threads. Async chat and pre-login events may query; the server-list ping may not.
  - JSON per packet or per block is the cost, not Rust: batch, send hot packets as bytes (`wire`), one
    `packets::flush` a tick.
  - Only send what changed (cache the last value per player): a name tag or score that stays the same costs nothing.
  - Placeholders and permission checks are queries: cache values per player for their refresh interval.
  - Disk and network on threads (`store` does it); write beside then rename.
  - An empty server pauses ticks: timers stop. Hold pausing off (`sync::hold`) while work is out.
  - The SDK's `World::get_chunk_at_async` is synchronous; for background chunk loads use force-load tickets.
  - The SDK's `BossBar` runs `/bossbar` commands (saved in the world, legacy titles): use `wire::boss`.
  - Editing horizon-kit while others build against it: change a copy, `cargo test` it, then copy in.
  - Bench against the Java plugin on Paper with a crowd (`plugin-sweep/audit/dh_crowd.py`), server thread vs server
    thread; time file reloads twice (the virus scanner slows the first read).
