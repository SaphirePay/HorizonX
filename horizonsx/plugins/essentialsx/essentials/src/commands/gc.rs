//! `Commandgc`.
use crate::prelude::*;
use crate::utils::number::format_double;
use horizon_bukkit::fam_core::runtime_memory;
use horizon_bukkit::serde_json::{Value, json};

pub const COMMAND: Command = Command { run_source: Some(run), ..Command::new("gc") };

/// HorizonsX: how long the chunk timing runs before the first answer, in ticks (it starts with the ask).
const FIRST_MEASURE_TICKS: u64 = 40;

fn run(sender: &CommandSource, _label: &str, _args: &[String]) -> EssResult {
    let tps = ess().timer_average_tps();
    let color = if tps >= 18.0 {
        "\u{a7}a"
    } else if tps >= 15.0 {
        "\u{a7}e"
    } else {
        "\u{a7}c"
    };
    sender.send_tl("uptime", crate::args![format_date_diff(ess().start_time())]);
    sender.send_tl("tps", crate::args![format!("{color}{}", format_double(tps))]);
    if let Some(memory) = runtime_memory() {
        const MB: u64 = 1024 * 1024;
        sender.send_tl("gcmax", crate::args![(memory.max / MB) as i64]);
        sender.send_tl("gctotal", crate::args![(memory.total / MB) as i64]);
        sender.send_tl("gcfree", crate::args![(memory.free / MB) as i64]);
    }
    for world in get_worlds() {
        let kind = match world.get_environment() {
            "NETHER" => "Nether",
            "THE_END" => "The End",
            _ => "World",
        };
        let chunks = world.get_loaded_chunks();
        let tiles: usize = chunks.iter().map(|chunk| chunk.get_tile_entities().len()).sum();
        sender.send_tl("gcWorld", crate::args![kind, world.get_name(), chunks.len(), world.get_entities().len(), tiles]);
    }
    if let Some(player) = sender.get_player() {
        chunk_line(player.clone());
    }
    Ok(())
}

/// The engine's name for a world's level (`World.dimension` is the SDK's own).
fn dimension_of(world: &World) -> String {
    match world.get_name() {
        "world" => "minecraft:overworld".to_owned(),
        "world_nether" => "minecraft:the_nether".to_owned(),
        "world_the_end" => "minecraft:the_end".to_owned(),
        name => horizon_bukkit::query_from_main(json!({ "op": "world_list" }))
            .as_array()
            .and_then(|rows| rows.iter().find(|row| row["name"] == name).and_then(|row| row["id"].as_str().map(str::to_owned)))
            .unwrap_or_else(|| "minecraft:overworld".to_owned()),
    }
}

/// HorizonsX: what the chunk the player stands in costs the server's tick (its mobs and block entities, timed by
/// the engine), as a line under the server's TPS. A chunk nobody asked about yet is timed from now: its line
/// comes two seconds later.
fn chunk_line(player: Player) {
    let location = player.get_location();
    let (x, z) = (location.get_block_x() >> 4, location.get_block_z() >> 4);
    let ask = {
        let dimension = dimension_of(&location.world);
        move || horizon_bukkit::query_from_main(json!({ "op": "chunk_cost", "dimension": dimension, "x": x, "z": z }))
    };
    let first = ask();
    if first.is_null() {
        // An engine without chunk timing.
        return;
    }
    if first["ms"].is_number() {
        send_chunk_line(&player, &location.world, x, z, &first);
        return;
    }
    let world = location.world.clone();
    ess().schedule_sync_delayed_task_later(
        move || {
            if player.is_online() {
                send_chunk_line(&player, &world, x, z, &ask());
            }
        },
        FIRST_MEASURE_TICKS,
    );
}

fn send_chunk_line(player: &Player, world: &World, x: i32, z: i32, cost: &Value) {
    let Some(ms) = cost["ms"].as_f64() else {
        return;
    };
    let chunk = world.get_chunk_at(x, z);
    let (entities, tiles) = (chunk.get_entities().len(), chunk.get_tile_entities().len());
    // A tick has 50 ms: the share of it this chunk takes.
    let share = ms / 50.0 * 100.0;
    let color = if ms < 1.0 {
        "#4ade80"
    } else if ms < 5.0 {
        "#facc15"
    } else {
        "#f87171"
    };
    let entity_word = if entities == 1 { "ᴇɴᴛɪᴛʏ" } else { "ᴇɴᴛɪᴛɪᴇs" };
    let tile_word = if tiles == 1 { "ᴛɪʟᴇ" } else { "ᴛɪʟᴇs" };
    let line = format!(
        "<#c4b5fd> • <#f4f4f5>ᴛʜɪs ᴄʜᴜɴᴋ <{color}><bold>{ms:.2} ᴍs</bold></{color}><#c4b5fd> ᴀ ᴛɪᴄᴋ · <#67e8f9>{share:.1}%<#c4b5fd> ᴏғ ᴛʜᴇ ᴛɪᴄᴋ · <#67e8f9>{entities}<#c4b5fd> {entity_word} · <#67e8f9>{tiles}<#c4b5fd> {tile_word} <#a1a1aa>({x}, {z})"
    );
    CommandSource::of_player(player.clone()).send_component(&deserialize_mini_message(&line));
}
