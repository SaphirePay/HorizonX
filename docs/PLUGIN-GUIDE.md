# Plugin guide

How to do the things plugins do, with the calls that do them. Each section names the [cookbook](COOKBOOK.md)
recipe that has complete, compiled code. For anything not here, ask the MCP tools (`sdk_can "words"`) or search
[`API_INDEX.md`](../sdk/horizon-bukkit/API_INDEX.md) for the Java name you know.

Contents:

1. [How the SDK is laid out](#1-how-the-sdk-is-laid-out)
2. [A plugin's life](#2-a-plugins-life)
3. [Commands](#3-commands)
4. [Events](#4-events)
5. [Timers](#5-timers)
6. [Text: chat, action bar, small caps](#6-text-chat-action-bar-small-caps)
7. [Items](#7-items)
8. [Hidden data on items (PDC)](#8-hidden-data-on-items-pdc)
9. [Inventories](#9-inventories)
10. [Blocks and the world](#10-blocks-and-the-world)
11. [Players: sounds, particles, fake blocks](#11-players-sounds-particles-fake-blocks)
12. [Config files](#12-config-files)
13. [Menus](#13-menus)
14. [Custom item icons from a resource pack](#14-custom-item-icons-from-a-resource-pack)
15. [Installing and updating](#15-installing-and-updating)
16. [Pitfalls](#16-pitfalls)
17. [Known gaps in the SDK](#17-known-gaps-in-the-sdk)

---

## 1. How the SDK is laid out

| Crate | Use it for |
|---|---|
| `horizon-bukkit` | The Bukkit/Paper API: `Player`, `World`, `Block`, `ItemStack`, `PlayerInventory`, events, `Plugin`. Java `getHealth()` is `get_health()`, `setHealth(20)` is `set_health(20.0)` |
| `horizon-kit` | Helpers on top: `app` (the plugin handle), `mini` (MiniMessage), `item` (names, lore, models), `inv` (giving items), `gui` and `window` (menus), `config`, `store` (saving data), `sound`, `smallcaps`, `time`, `sync`. See [KIT.md](../sdk/horizon-kit/KIT.md) |

Most types are at the crate root (`horizon_bukkit::Player`). Events live in modules by family:
`horizon_bukkit::event` (join, quit, chat...), `fam_ev_player`, `fam_ev_entity`, `fam_ev_world`, `fam_ev_inventory`.
Some helpers sit in other modules: right-click detection is `horizon_bukkit::core2::action_is_right_click`. When you
don't know where something lives, `sdk_doc <name>` gives its exact `use` path.

The SDK has 11,500+ functions and 308 listenable events. Don't read it top to bottom: search it.

## 2. A plugin's life

`bukkit_plugin!(on_enable, on_disable)` names your two entry points. In `on_enable`:

- `app::set(plugin)` first, so `app::info`, `app::warn`, `app::folder()` and `app::later` work from anywhere.
- `gui::install(plugin)` and `window::install(plugin)` if you open menus.
- Register commands, events and timers.

In `on_disable`, undo anything that would outlive the plugin: blocks you changed for a moment, fake blocks you sent,
open menus (`gui::close_all()`). Then `app::clear()`. Plugins are reloaded while the server runs, so a clean disable
matters more than on Paper.

Recipe: [skeleton](COOKBOOK.md#skeleton).

## 3. Commands

Declare the command in `plugin.yml` (name, description, aliases, permission), then:

```rust
plugin.set_executor("relic", |sender, _label, args| run(sender, args));   // args: &[&str]
plugin.set_tab_completer("relic", |_, _, args| match args.len() {
    1 => vec!["give".into(), "list".into()],
    _ => Vec::new(),
});
```

`sender.as_player()` is `Some(&Player)` when a player ran it. Reply with
`sender.send_message(&horizon_kit::mini::legacy("<green>Done"))`.

Recipe: [command](COOKBOOK.md#command).

## 4. Events

```rust
plugin.register_event_with(enums::event_priority::HIGH, false, |e: &mut PlayerInteractEvent| {
    // read e, change it, or e.set_cancelled(true)
});
```

- **Priority** works as on Paper: `LOWEST` runs first, `MONITOR` last. Use `MONITOR` only to watch the final result,
  never to change it.
- **The second argument** is `ignoreCancelled`: `true` skips events another plugin already cancelled. With `false`,
  check `e.is_cancelled()` yourself before acting (protection plugins cancel breaks in claimed land).
- **Check the event fires before you rely on it.** 299 of the 308 events are fired by the engine. `sdk_fires <Event>`
  says where; `sdk_map` lists the ones that never fire (today: the four profile events, two chat-preview/tab
  events, `EntityCreatePortalEvent` and the two service events).
- **Some events share one engine hook.** `EndermanAttackPlayerEvent` rides the same hook as `EntityTargetEvent`. To stop
  one specific case, listen to the general event and filter: `EntityTargetEvent` with `get_entity_type()` and
  `get_reason()` (`CLOSEST_PLAYER` for an enderman's stare, `TARGET_ATTACKED_ENTITY` when it was hit).
- **Changing values works like Paper:** `EntityExhaustionEvent::set_exhaustion(x)` scales hunger,
  `BlockBreakEvent::set_drop_items(false)` stops drops.

Recipes: [right_click](COOKBOOK.md#right-click), [extra_drops](COOKBOOK.md#extra-drops), [hunger](COOKBOOK.md#hunger),
[mob_target](COOKBOOK.md#mob-target).

## 5. Timers

```rust
plugin.run_task_timer(20, 20, || { /* every second (20 ticks) */ });
app::later(40, || { /* once, two seconds from now */ });
```

Timers run on the server thread, so anything you do in them happens in order with the game. Keep per-tick work small:
a timer that reads every player's whole inventory every tick shows up in the tick time. Cache what you can and look
again every second or when something changes.

Recipe: [timers](COOKBOOK.md#timers).

## 6. Text: chat, action bar, small caps

All text is MiniMessage: `<#a855f7>`, `<bold>`, `<gradient:#a855f7:#ec4899>`, `<click:run_command:'/spawn'>`,
`<hover:show_text:'...'>`.

| Where | Call |
|---|---|
| Chat | `player.send_message(&horizon_kit::mini::legacy("<green>Hi"))` |
| Action bar | `player.send_action_bar(&horizon_bukkit::mini_message("<green>Active"))` |
| Item names and lore | `horizon_kit::item::name(&mut stack, "<gold>Name")`, `item::lore(&mut stack, &lines)` |
| Small caps (ᴛʜɪꜱ ꜱᴛʏʟᴇ) | `horizon_kit::smallcaps::small("text")` |

Names and lore set through `item::` are not italic, unlike raw lore on Paper.

Recipes: [chat](COOKBOOK.md#chat), [action_bar](COOKBOOK.md#action-bar).

## 7. Items

```rust
let mut s = ItemStack::new(Material::of("CLOCK"), 1);
item::name(&mut s, "<#86efac><bold>Hearthroot Relic");
item::lore(&mut s, &lines);
s.set_component("minecraft:item_model", json!("myicons:relic"));     // an icon from the server pack
s.set_component("minecraft:enchantment_glint_override", json!(true)); // the enchanted shimmer
s.set_component("minecraft:max_stack_size", json!(1));
s.set_component("minecraft:tooltip_display", json!({ "hidden_components": ["minecraft:enchantments"] }));
horizon_kit::inv::give(&player, &s);
```

Any 26.3 data component can be set by its id with `set_component`; `get_component` reads one back. Useful ones:
`item_model`, `enchantment_glint_override`, `max_stack_size`, `tooltip_display` (hide vanilla lines),
`enchantments`, `attribute_modifiers`, `equippable`, `unbreakable`, `damage`, `rarity`, `custom_model_data`.

**Choose the base material with care.** The item still is that material to the game: an amethyst shard works in
recipes, a compass points, food can be eaten. Pick one that does nothing on its own (a clock is a good base for a
"right-click to use" item) and cancel its normal use in your interact listener.

Recipe: [custom_item](COOKBOOK.md#custom-item).

## 8. Hidden data on items (PDC)

Tag your items so you can recognise them later, and store per-item values (a rolled stat, an owner):

```rust
let key = NamespacedKey::from_string("myplugin:id").expect("key");
stack.pdc_set(&key, json!("hearthroot"));
let id = stack.pdc_get(&key).and_then(|v| v.as_str().map(str::to_owned));
```

Values are saved as NBT, and they come back as NBT types:

- **`true` comes back as the number `1`** (an NBT byte), not as a JSON bool. Read flags with
  `matches!(v, Value::Bool(true)) || v.as_i64() == Some(1)`.
- Whole numbers come back as numbers; decimals may come back as numbers or as text like `"12.5d"`. Read them
  defensively.

Recipe: [item_tag](COOKBOOK.md#item-tag).

## 9. Inventories

Slot numbers for `PlayerInventory::set_item_at(slot, Some(&stack))`:

| Slots | What |
|---|---|
| 0-8 | Hotbar |
| 9-35 | The inventory rows |
| 36, 37, 38, 39 | Boots, leggings, chestplate, helmet |
| 40 | Off hand |

- `get_contents()` lists slots 0-35 in that order, then more; take `.take(36)` for the main inventory.
- `get_held_item_slot()` is the selected hotbar slot; `get_item_in_main_hand()`, `get_boots()` and the like read
  single slots.
- `set_item_at(slot, None)` empties a slot.
- To give an item where it fits, use `horizon_kit::inv::give(&player, &stack)`.

Recipe: [inventory_slots](COOKBOOK.md#inventory-slots).

## 10. Blocks and the world

```rust
let world = player.get_location().get_world();
let b = world.get_block_at(x, y, z);
let kind = b.get_type().key();        // "minecraft:wheat"
let state = b.get_block_data();       // "minecraft:wheat[age=3]"
b.set_block_data("minecraft:wheat[age=4]");
```

Block states are text, exactly as in commands. To change one property, edit the text (see the
[crop_age](COOKBOOK.md#crop-age) recipe). Dropping an item: `world.drop_item_naturally(&location, &stack)`.

Reading blocks is fast, but a few hundred per tick per player adds up: scan an area every second or two rather than
every tick.

Recipes: [blocks_near](COOKBOOK.md#blocks-near), [crop_age](COOKBOOK.md#crop-age).

## 11. Players: sounds, particles, fake blocks

| Want | Call |
|---|---|
| A sound to one player | `horizon_kit::sound::play(&player, "BLOCK_AMETHYST_BLOCK_CHIME:1:1.3")` (name:volume:pitch) |
| A sound at a place | `player.play_sound(&location, "minecraft:entity.experience_orb.pickup", 0.6, 1.4)` |
| Particles | `world.spawn_particle("happy_villager", &location, count, dx, dy, dz, speed)` |
| A block only one player sees | `player.send_block_change(&location, "minecraft:snow_block")` |

Fake blocks stay until the real block is sent again: send the real `get_block_data()` back when you are done, and in
`on_disable`.

Recipes: [sound](COOKBOOK.md#sound), [particles](COOKBOOK.md#particles), [fake_blocks](COOKBOOK.md#fake-blocks).

## 12. Config files

Ship a default with the plugin (`include_str!("../config.yml")`), write it on first start, read it with
`horizon_kit::config::Config::parse(&text)`: `c.str("path", "default")`, `c.int(...)`, `c.bool(...)`,
`c.keys("section")`, `c.list("path")`. Re-read it in a `reload` sub-command.

Recipe: [config](COOKBOOK.md#config).

## 13. Menus

```rust
let mut m = window::menu(&player, 3, "Relics");          // 3 rows, themed window
m.fill(gui::filler("BLACK_STAINED_GLASS_PANE"));
m.button(13, stack, |c| { /* c.player clicked; c.click.is_right() */ });
m.button(26, gui::icon("BARRIER", "<red>Close", &[]), |c| c.close());
m.open(&player);
```

Menus need `gui::install(plugin)` and `window::install(plugin)` in `on_enable`. Pixel-placed menus drawn by a
resource pack have their own tools (the `uui_*` MCP tools).

Recipe: [menu](COOKBOOK.md#menu).

## 14. Custom item icons from a resource pack

An `item_model` id `myicons:relic` needs three files in the server's resource pack:

```
assets/myicons/items/relic.json            {"model":{"type":"minecraft:model","model":"myicons:item/relic"}}
assets/myicons/models/item/relic.json      {"parent":"minecraft:item/generated","textures":{"layer0":"myicons:item/relic"}}
assets/myicons/textures/item/relic.png     the picture (16x16 or 32x32, RGBA)
```

After changing the pack, update its `sha1` wherever the server announces it and have the plugin that sends it
reload. Players download the new pack. A client that was mid-join while the pack changed can keep the old one until
it rejoins, so test with a fresh join.

## 15. Installing and updating

- Build: `cargo build --release`. Windows makes `name.dll`, Linux `libname.so`.
- Install or update: copy into `plugins/` with a new modified time; the server swaps it in while it runs.
- Plugin data lives in `plugins/<Name>/` (from `plugin.yml`), found with `app::folder()`.
- Changing a shipped default file (like `items.yml`) does not change the copy already in a server's folder: replace
  that copy too, after checking nobody edited it.

## 16. Pitfalls

- **An event that never fires** compiles fine. Check with `sdk_fires` first.
- **PDC booleans read back as `1`.** See [section 8](#8-hidden-data-on-items-pdc).
- **An item's power should not follow its base material.** If a plugin gives powers for "the item in hand", check
  the item's own state too (for example "is this relic switched on?"), or holding it is enough to get the power.
- **Spawn protection** undoes block changes in protected worlds. Test block plugins in a world without protection.
- **Per-tick work** shows up in TPS. Cache, and spread big scans over time.
- **Small-caps text** has no small-caps digits: numbers stay normal height, which is usually what you want for stats.

## 17. Known gaps in the SDK

What is still rough, so you don't lose time on it:

| Gap | Work-around |
|---|---|
| PDC booleans come back as bytes | Read `1`/`true`/`"1b"` as true |
| `EndermanAttackPlayerEvent` shares the target hook | Listen to `EntityTargetEvent` and filter by entity type and reason |
| 9 events never fire | `sdk_map` lists them; pick another event |
| Some helpers live in less obvious modules (`core2::action_is_right_click`) | `sdk_doc <name>` gives the `use` path |
| Block states are plain text | Edit the `[key=value]` text; see the crop recipe |
| `set_item_at` runs a command under the hood | Fine for a few slots; batch big inventory changes |
| Particles and sounds are names as text | Copy names from the cookbook or the game's own lists; a typo fails quietly |
