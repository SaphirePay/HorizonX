//! The Java layer: Bukkit/Spigot/Paper plugins from `plugins/*.jar`, unchanged, on the Rust server.
//!
//! A real Java VM (the server owner's Java 25+) runs the jars. The Bukkit API is the real Paper API jar; its
//! interfaces (`Player`, `World`, `Block`...) are answered by `horizon.javalayer` (java/), which hands every call
//! here: [`natives`] runs it on the SDK through the same Bukkit-name table jport's converted plugins use
//! ([`generated`]). Events go the other way ([`events`]): an SDK listener per Bukkit event class some Java plugin
//! listens to, which hands the event to Java.
//!
//! Nothing starts when `plugins/` has no jar: no Java, no memory.
mod adapters;
mod components_gen;
mod conv;
mod events;
#[allow(clippy::all)]
mod generated;
mod jvm;
mod natives;

use horizon_bukkit::{Plugin, bukkit_plugin};

fn on_enable(plugin: Plugin) {
    let data = plugin.get_data_folder();
    let Some(plugins_dir) = data.parent().map(std::path::Path::to_path_buf) else {
        plugin.severe("no plugins folder");
        return;
    };
    let jars = jvm::plugin_jars(&plugins_dir);
    if jars.is_empty() {
        plugin.info("No Java plugins (plugins/*.jar): Java is not started.");
        return;
    }
    // The game's version, asked here on the game thread so plugin threads read it without waiting for the game.
    let _ = horizon_bukkit::server::get_minecraft_version();
    jbridge::start(plugin.clone(), &[], |_, _| {});
    jrt::set_hooks(generated::hooks(natives::layer_call, natives::layer_instance));
    // Packet libraries' handlers (PacketEvents) see the game's packets once Java asks for them (`raw:out`).
    events::listen_raw_packets();
    let started = std::time::Instant::now();
    match jvm::start(&plugin, &plugins_dir, &jars) {
        Ok(()) => plugin.info(&format!("Java layer up: {} jar(s) in {} ms", jars.len(), started.elapsed().as_millis())),
        Err(error) => plugin.severe(&format!("Java plugins not loaded: {error}")),
    }
}

fn on_disable(_plugin: Plugin) {
    jvm::stop();
}

bukkit_plugin!(on_enable, on_disable);
