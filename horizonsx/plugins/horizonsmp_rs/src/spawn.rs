//! `/horizonsmp setupspawn [radius]` (console or staff): a spawn plaza built at the world spawn, the three crates
//! on pedestals around it, and the land around it made server land (anyone may use doors and buttons, nobody may
//! build). Safe to run again: it rebuilds the plaza and re-places the crates.

use crate::{claims, crates, ui};
use horizon_bukkit::prelude::*;

const SECTION: &str = "Spawn";

fn set(w: &World, x: i32, y: i32, z: i32, material: &str) {
    if let Some(m) = Material::get_material(material) {
        w.get_block_at(x, y, z).set_type(&m);
    }
}

/// The floor's block at (dx, dz) from the centre, by distance: Nyxora's purple and cyan rings on dark stone.
fn floor(d: f64) -> &'static str {
    if d < 1.6 {
        "SEA_LANTERN"
    } else if (3.0..3.9).contains(&d) {
        "PURPUR_BLOCK"
    } else if (6.6..7.5).contains(&d) {
        "CYAN_TERRACOTTA"
    } else if d >= 9.1 {
        "DEEPSLATE_TILES"
    } else {
        "POLISHED_DEEPSLATE"
    }
}

/// The ground at a column: the highest block that isn't a tree, a plant or snow.
fn ground(w: &World, x: i32, z: i32) -> i32 {
    let mut y = w.get_highest_block_y_at(x, z);
    for _ in 0..40 {
        let name = w.get_block_at(x, y, z).get_type().name();
        let soft = name.contains("LEAVES") || name.ends_with("_LOG") || name.ends_with("_WOOD") || name.contains("AIR") || name == "SNOW" || name.contains("VINE") || name.contains("GRASS") && name != "GRASS_BLOCK" || name.contains("FERN") || name.contains("FLOWER") || name == "BAMBOO";
        if !soft {
            break;
        }
        y -= 1;
    }
    y
}

pub fn setup(sender: &CommandSender, args: &[&str]) {
    let Some(w) = horizon_bukkit::get_worlds().into_iter().next() else {
        return ui::tell(sender, SECTION, "<#f87171>No world is loaded.");
    };
    let protect = args.first().and_then(|a| a.parse::<i32>().ok()).unwrap_or(48).clamp(12, 256);
    // In game: where the staff member stands (their world, their feet). From the console: the world spawn, and only
    // with someone near it, since blocks of chunks nobody has loaded read as air.
    let (w, cx, cz, y) = match sender.as_player() {
        Some(p) => {
            let l = p.get_location();
            (l.get_world().clone(), l.get_block_x(), l.get_block_z(), l.get_block_y() - 1)
        }
        None => {
            let spawn = w.get_spawn_location();
            let (cx, cz) = (spawn.get_block_x(), spawn.get_block_z());
            let near = horizon_bukkit::get_online_players().iter().any(|p| {
                let l = p.get_location();
                l.get_world().get_name() == w.get_name() && (l.get_block_x() - cx).abs() < 48 && (l.get_block_z() - cz).abs() < 48
            });
            if !near {
                return ui::tell(sender, SECTION, "<#f87171>Nobody is near the spawn, so its chunks aren't loaded. Run it in game where the plaza should go, or with someone at spawn.");
            }
            let y = ground(&w, cx, cz);
            if y < spawn.get_block_y() - 24 {
                return ui::tell(sender, SECTION, &format!("<#f87171>The ground at spawn reads as y {y}, far below the spawn point (y {}): not building there.", spawn.get_block_y()));
            }
            (w, cx, cz, y)
        }
    };
    let r = 10;
    let mut placed = 0;
    for dx in -r..=r {
        for dz in -r..=r {
            let d = f64::from(dx * dx + dz * dz).sqrt();
            if d > f64::from(r) + 0.4 {
                continue;
            }
            let (x, z) = (cx + dx, cz + dz);
            // A solid base under the floor, and room to stand above it.
            for below in 1..=3 {
                if w.get_block_at(x, y - below, z).get_type().is_air() {
                    set(&w, x, y - below, z, "DEEPSLATE");
                }
            }
            set(&w, x, y, z, floor(d));
            for up in 1..=7 {
                set(&w, x, y + up, z, "AIR");
            }
            placed += 1;
        }
    }
    // Lamp posts around the edge.
    for i in 0..8 {
        let a = f64::from(i) * std::f64::consts::FRAC_PI_4 + std::f64::consts::FRAC_PI_8;
        let (x, z) = (cx + (a.cos() * 9.0).round() as i32, cz + (a.sin() * 9.0).round() as i32);
        set(&w, x, y + 1, z, "POLISHED_BLACKSTONE_WALL");
        set(&w, x, y + 2, z, "POLISHED_BLACKSTONE_WALL");
        set(&w, x, y + 3, z, "LANTERN");
    }
    // The crates on gold pedestals, north, east and west of the centre (south is the way in).
    let spots = [(0, -6), (6, 0), (-6, 0), (0, 6)];
    let blocks = ["CHEST", "ENDER_CHEST", "VAULT", "BARREL"];
    let all = crates::all();
    crates::remove_near(w.get_name(), cx, cz, r);
    for (i, c) in all.iter().take(4).enumerate() {
        let (dx, dz) = spots[i];
        let (x, z) = (cx + dx, cz + dz);
        set(&w, x, y, z, "GOLD_BLOCK");
        let block = if Material::get_material(blocks[i]).is_some() { blocks[i] } else { "CHEST" };
        set(&w, x, y + 1, z, block);
        crates::place(w.get_name(), x, y + 1, z, &c.id);
    }
    let land = match claims::server_claim(w.get_name(), cx - protect, cz - protect, cx + protect, cz + protect) {
        Ok(c) => format!("server land {}×{}", c.width(), c.length()),
        Err(e) => format!("<#facc15>no new server land ({e})"),
    };
    ui::tell(sender, SECTION, &format!("<#4ade80>Spawn built</#4ade80> at <white>{cx}, {y}, {cz}</white>: {placed} floor blocks, {} crates, {land}.", all.len().min(4)));
}
