# Cookbook

Working code for the jobs most plugins do. Every recipe here is a function in
[`sdk/cookbook/src/lib.rs`](../sdk/cookbook/src/lib.rs), and that crate is compiled against this SDK, so a recipe
that stopped compiling would be caught. The MCP tool `sdk_recipe <task>` hands out the same code by words
(`sdk_recipe "right click item"`).

Add a recipe: write the function under a `/// recipe: key | Title | words to find it by` line, then run
`cargo check` in `sdk/cookbook`.

## Recipes

- [A whole plugin: enable, disable, command, event, timer](#skeleton)
- [A command with sub-commands, player-only parts and a usage line](#command)
- [A MiniMessage chat line (colours, bold, click and hover)](#chat)
- [Text over the hotbar](#action-bar)
- [A sound to one player (`NAME:volume:pitch`) or at a place for everyone near](#sound)
- [Particles in the world at a block](#particles)
- [A custom item: name, lore, pack model, glint, stack size and a hidden id tag](#custom-item)
- [Read the hidden id tag back off an item](#item-tag)
- [Every inventory slot, and replacing one (0-8 hotbar, 9-35 inside, 36-39 armour, 40 offhand)](#inventory-slots)
- [Right-clicking with a custom item (air or block), main hand only, stopping its normal use](#right-click)
- [Run later, every N ticks, and per player each tick](#timers)
- [A YAML file in the plugin folder, written from a default on first start](#config)
- [A chest menu: filler, an item that does something, close button](#menu)
- [Read and change blocks around a player (states as text: `minecraft:wheat[age=3]`)](#blocks-near)
- [Grow a crop one stage: read `age=` from the block state and write it back](#crop-age)
- [More drops when a player breaks a block (after protection plugins had their say)](#extra-drops)
- [Slower hunger: scale the exhaustion a player is about to take](#hunger)
- [Stop a mob targeting a player for one reason only (an enderman's stare, not when hit)](#mob-target)
- [Show one player a block that isn't really there (per-player)](#fake-blocks)

## Imports used by the recipes

```rust
use horizon_bukkit::event::PlayerQuitEvent;
use horizon_bukkit::fam_ev_entity::{EntityExhaustionEvent, EntityTargetEvent};
use horizon_bukkit::fam_ev_player::PlayerInteractEvent;
use horizon_bukkit::fam_ev_world::BlockBreakEvent;
use horizon_bukkit::persistence::NamespacedKey;
use horizon_bukkit::{CommandSender, ItemStack, Location, Material, Player, Plugin, bukkit_plugin, enums, mini_message};
use horizon_kit::config::Config;
use horizon_kit::{app, gui, item, sound, window};
use serde_json::json;
```

## skeleton

**A whole plugin: enable, disable, command, event, timer.** Found by: start, new, plugin, enable, disable, lib.

```rust
fn on_enable(plugin: Plugin) {
    app::set(plugin); // app::get / folder / info / warn / later from anywhere
    gui::install(plugin); // chest menus (needed once if the plugin opens any)
    window::install(plugin); // the network's themed menu window
    plugin.set_executor("example", |s, _label, args| command(s, args));
    plugin.set_tab_completer("example", |_, _, args| match args.len() {
        1 => vec!["give".into(), "reload".into()],
        _ => Vec::new(),
    });
    plugin.register_event_with(enums::event_priority::MONITOR, false, |e: &mut PlayerQuitEvent| {
        app::info(&format!("{} left", e.get_player().get_name()));
    });
    plugin.run_task_timer(20, 20, || { /* every second */ });
    app::info("Example ready");
}

fn on_disable(_plugin: Plugin) {
    gui::close_all();
    app::clear();
}

bukkit_plugin!(on_enable, on_disable);
```

## command

**A command with sub-commands, player-only parts and a usage line.** Found by: command, executor, args, sub, permission.

```rust
fn command(sender: &CommandSender, args: &[&str]) -> bool {
    match args.first().map(|a| a.to_ascii_lowercase()).as_deref() {
        Some("give") => {
            let Some(p) = sender.as_player() else {
                sender.send_message("Players only.");
                return true;
            };
            horizon_kit::inv::give(p, &custom_item());
        }
        Some("reload") => sender.send_message(&horizon_kit::mini::legacy("<#4ade80>Reloaded.")),
        _ => sender.send_message(&horizon_kit::mini::legacy("<white>/example give · /example reload")),
    }
    true
}
```

## chat

**A MiniMessage chat line (colours, bold, click and hover).** Found by: chat, message, send, minimessage, colour, hover, click.

```rust
fn chat(p: &Player) {
    p.send_message(&horizon_kit::mini::legacy("<#a855f7><bold>Items</bold> <#c4b5fd>» <click:run_command:'/spawn'><u>go to spawn</u></click>"));
}
```

## action-bar

**Text over the hotbar.** Found by: action, bar, actionbar, hotbar, status.

```rust
fn action_bar(p: &Player) {
    p.send_action_bar(&mini_message("<#4ade80>● <white>Relic active"));
}
```

## sound

**A sound to one player (`NAME:volume:pitch`) or at a place for everyone near.** Found by: sound, play, noise.

```rust
fn play_sounds(p: &Player) {
    sound::play(p, "BLOCK_AMETHYST_BLOCK_CHIME:1:1.2");
    let at = p.get_location();
    p.play_sound(&at, "minecraft:entity.experience_orb.pickup", 0.6, 1.4);
}
```

## particles

**Particles in the world at a block.** Found by: particle, spawn, effect, sparkle.

```rust
fn particles(p: &Player) {
    let at = p.get_location();
    let world = at.get_world();
    let spot = Location::new(world.clone(), at.get_x(), at.get_y() + 1.0, at.get_z(), 0.0, 0.0);
    // name, where, count, spread x/y/z, speed
    world.spawn_particle("happy_villager", &spot, 8, 0.3, 0.3, 0.3, 0.0);
}
```

## custom-item

**A custom item: name, lore, pack model, glint, stack size and a hidden id tag.** Found by: item, custom, name, lore, model, icon, glint, pdc, tag, stack, component.

```rust
fn custom_item() -> ItemStack {
    let mut s = ItemStack::new(Material::of("CLOCK"), 1);
    item::name(&mut s, "<#86efac><bold>Hearthroot Relic</bold> <#60a5fa>★★★<#3f3f46>★★");
    item::lore(&mut s, &["".to_owned(), "<#86efac>┃ <#d4d4d8>Walk across your farms.".to_owned()]);
    s.set_component("minecraft:item_model", json!("zenith:relic_filigree")); // an items/<id>.json in the server pack
    s.set_component("minecraft:enchantment_glint_override", json!(true));
    s.set_component("minecraft:max_stack_size", json!(1));
    s.set_component("minecraft:tooltip_display", json!({ "hidden_components": ["minecraft:enchantments"] }));
    s.pdc_set(&NamespacedKey::from_string("example:id").expect("key"), json!("hearthroot"));
    s
}
```

## item-tag

**Read the hidden id tag back off an item.** Found by: pdc, tag, persistent, data, read, item, id, which, item.

```rust
fn item_id(stack: Option<&ItemStack>) -> Option<String> {
    let s = stack?;
    if s.get_type().is_air() {
        return None;
    }
    s.pdc_get(&NamespacedKey::from_string("example:id").expect("key")).and_then(|v| v.as_str().map(str::to_owned))
}
```

## inventory-slots

**Every inventory slot, and replacing one (0-8 hotbar, 9-35 inside, 36-39 armour, 40 offhand).** Found by: inventory, slot, contents, replace, held, hand, armour, offhand.

```rust
fn inventory_slots(p: &Player) {
    let inv = p.get_inventory();
    for (slot, stack) in inv.get_contents().into_iter().enumerate().take(36) {
        if item_id(stack.as_ref()).is_some() {
            inv.set_item_at(slot as i32, Some(&custom_item()));
        }
    }
    let held = inv.get_held_item_slot(); // the hotbar slot in hand
    let _in_hand = inv.get_item_in_main_hand();
    let _boots = inv.get_boots();
    inv.set_item_at(held, None); // empty it
}
```

## right-click

**Right-clicking with a custom item (air or block), main hand only, stopping its normal use.** Found by: right, click, interact, use, item, hand, cancel, activate.

```rust
fn right_click(plugin: Plugin) {
    plugin.register_event_with(enums::event_priority::HIGH, false, |e: &mut PlayerInteractEvent| {
        if !horizon_bukkit::core2::action_is_right_click(e.get_action()) || e.get_hand() != Some("HAND") {
            return;
        }
        if item_id(e.get_item().as_ref()).is_some() {
            e.set_cancelled(true); // no placing, eating or using the base item
            action_bar(e.get_player());
        }
    });
}
```

## timers

**Run later, every N ticks, and per player each tick.** Found by: timer, later, delay, repeat, schedule, tick, task.

```rust
fn timers(plugin: Plugin) {
    app::later(40, || app::info("two seconds later"));
    plugin.run_task_timer(1, 1, || {
        for p in horizon_bukkit::get_online_players() {
            let _ = p.get_location(); // per player, every tick: keep it cheap
        }
    });
}
```

## config

**A YAML file in the plugin folder, written from a default on first start.** Found by: config, yaml, file, settings, load, reload.

```rust
const DEFAULT_CONFIG: &str = "speed: 2\nworlds:\n  - world\n";
fn config() -> (i64, Vec<String>) {
    let path = app::folder().join("config.yml");
    if !path.exists() {
        let _ = std::fs::create_dir_all(app::folder());
        let _ = std::fs::write(&path, DEFAULT_CONFIG);
    }
    let c = Config::parse(&std::fs::read_to_string(&path).unwrap_or_default());
    (c.int("speed", 1), c.keys("worlds"))
}
```

## menu

**A chest menu: filler, an item that does something, close button.** Found by: gui, menu, chest, inventory, button, click, page.

```rust
fn menu(p: &Player) {
    let mut m = window::menu(p, 3, "Relics");
    m.fill(gui::filler("BLACK_STAINED_GLASS_PANE"));
    m.button(13, custom_item(), |c| {
        horizon_kit::inv::give(&c.player, &custom_item());
        sound::play(&c.player, "ENTITY_ITEM_PICKUP:0.6:1.2");
    });
    m.button(26, gui::icon("BARRIER", "<#f87171><bold>Close", &[]), |c| c.close());
    m.open(p);
}
```

## blocks-near

**Read and change blocks around a player (states as text: `minecraft:wheat[age=3]`).** Found by: block, blocks, around, near, scan, set, data, state.

```rust
fn blocks_near(p: &Player) {
    let at = p.get_location();
    let world = at.get_world();
    let (bx, by, bz) = (at.get_block_x(), at.get_block_y(), at.get_block_z());
    for dx in -4..=4 {
        for dz in -4..=4 {
            for dy in -2..=2 {
                let b = world.get_block_at(bx + dx, by + dy, bz + dz);
                if b.get_type().key() == "minecraft:stone" {
                    b.set_block_data("minecraft:mossy_cobblestone");
                }
            }
        }
    }
}
```

## crop-age

**Grow a crop one stage: read `age=` from the block state and write it back.** Found by: crop, age, grow, ageable, wheat, carrot, farm, bonemeal.

```rust
fn grow_one_stage(world: &horizon_bukkit::World, x: i32, y: i32, z: i32) -> bool {
    let b = world.get_block_at(x, y, z);
    let data = b.get_block_data(); // e.g. minecraft:wheat[age=3]
    let max = match b.get_type().key() {
        "minecraft:wheat" | "minecraft:carrots" | "minecraft:potatoes" | "minecraft:melon_stem" | "minecraft:pumpkin_stem" => 7,
        "minecraft:beetroots" | "minecraft:nether_wart" | "minecraft:sweet_berry_bush" => 3,
        "minecraft:cocoa" => 2,
        _ => return false,
    };
    let Some(i) = data.find("age=") else { return false };
    let digits: String = data[i + 4..].chars().take_while(char::is_ascii_digit).collect();
    let Ok(age) = digits.parse::<u32>() else { return false };
    if age >= max {
        return false;
    }
    let next = format!("{}age={}{}", &data[..i], age + 1, &data[i + 4 + digits.len()..]);
    b.set_block_data(&next)
}
```

## extra-drops

**More drops when a player breaks a block (after protection plugins had their say).** Found by: break, block, drops, loot, extra, leaves, ore, mine.

```rust
fn extra_drops(plugin: Plugin) {
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut BlockBreakEvent| {
        if e.is_cancelled() || !e.is_drop_items() {
            return;
        }
        let b = e.get_block();
        if b.get_type().key() == "minecraft:oak_leaves" {
            let at = b.get_location();
            at.get_world().drop_item_naturally(&at, &ItemStack::new(Material::of("APPLE"), 1));
        }
    });
}
```

## hunger

**Slower hunger: scale the exhaustion a player is about to take.** Found by: hunger, food, exhaustion, saturation, starve, eat.

```rust
fn slower_hunger(plugin: Plugin) {
    plugin.register_event_with(enums::event_priority::NORMAL, false, |e: &mut EntityExhaustionEvent| {
        let Some(actor) = e.get_entity() else { return };
        if actor.as_player().is_some() {
            let less = e.get_exhaustion() * 0.7;
            e.set_exhaustion(less);
        }
    });
}
```

## mob-target

**Stop a mob targeting a player for one reason only (an enderman's stare, not when hit).** Found by: mob, target, aggro, enderman, angry, hostile, reason.

```rust
fn calm_endermen(plugin: Plugin) {
    plugin.register_event_with(enums::event_priority::NORMAL, false, |e: &mut EntityTargetEvent| {
        if e.get_entity_type() != "minecraft:enderman" || e.get_reason() != "CLOSEST_PLAYER" {
            return; // TARGET_ATTACKED_ENTITY (it was hit) still goes through
        }
        if e.get_target().as_ref().and_then(|t| t.as_player()).is_some() {
            e.set_cancelled(true);
        }
    });
}
```

## fake-blocks

**Show one player a block that isn't really there (per-player).** Found by: fake, block, per, player, client, side, show, send, block, change.

```rust
fn fake_block(p: &Player) {
    let at = p.get_location();
    let below = Location::new(at.get_world().clone(), f64::from(at.get_block_x()), f64::from(at.get_block_y() - 1), f64::from(at.get_block_z()), 0.0, 0.0);
    p.send_block_change(&below, "minecraft:snow_block");
}
```

