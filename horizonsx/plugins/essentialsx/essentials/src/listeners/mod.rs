//! `EssentialsPlayerListener`, `EssentialsBlockListener`, `EssentialsEntityListener` and the rest.
//! Every handler is registered with its Java `@EventHandler`'s priority and `ignoreCancelled`, and
//! a Java handler is a registration of its own, so two at different priorities stay two. GAPS.md
//! lists each handler that has no event yet.
pub mod block;
pub mod entity;
pub mod player;

use horizon_bukkit::Plugin;

/// `registerListeners`.
pub fn register(plugin: Plugin) {
    player::register(plugin);
    block::register(plugin);
    entity::register(plugin);
    crate::signs::listeners::register(plugin);
}
