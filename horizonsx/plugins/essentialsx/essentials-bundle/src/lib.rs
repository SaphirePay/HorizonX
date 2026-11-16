//! EssentialsX and its modules as the one library the server loads. GPL-3.0: see NOTICE.
//!
//! A plugin here is a library of its own, with its own copy of everything static, and one can't
//! call into another. A module works on the live `User`s of Essentials, so they're linked together
//! here; each module is still its own crate and its own feature. GAPS.md has the rest of that.
use horizon_bukkit::{Plugin, bukkit_plugin};

fn on_enable(plugin: Plugin) {
    essentials::on_enable(plugin);
    // In the order Paper lists them, which is what `/help` and `/essentials version` show.
    #[cfg(feature = "chat")]
    essentials::add_module("EssentialsChat");
    #[cfg(feature = "discord")]
    essentials::add_module("EssentialsDiscord");
    #[cfg(feature = "spawn")]
    essentials::add_module("EssentialsSpawn");
    #[cfg(feature = "xmpp")]
    essentials::add_module("EssentialsXMPP");
    #[cfg(feature = "protect")]
    essentials::add_module("EssentialsProtect");
    #[cfg(feature = "geoip")]
    essentials::add_module("EssentialsGeoIP");
    #[cfg(feature = "discordlink")]
    essentials::add_module("EssentialsDiscordLink");
    #[cfg(feature = "antibuild")]
    essentials::add_module("EssentialsAntiBuild");
    #[cfg(feature = "chat")]
    essentials_chat::on_enable(plugin);
    #[cfg(feature = "spawn")]
    essentials_spawn::on_enable(plugin);
    #[cfg(feature = "antibuild")]
    essentials_antibuild::on_enable(plugin);
    #[cfg(feature = "protect")]
    essentials_protect::on_enable(plugin);
    #[cfg(feature = "geoip")]
    essentials_geoip::on_enable(plugin);
    #[cfg(feature = "xmpp")]
    essentials_xmpp::on_enable(plugin);
    #[cfg(feature = "discord")]
    essentials_discord::on_enable(plugin);
    #[cfg(feature = "discordlink")]
    essentials_discordlink::on_enable(plugin);
}

fn on_disable(plugin: Plugin) {
    #[cfg(feature = "discordlink")]
    essentials_discordlink::on_disable(plugin);
    #[cfg(feature = "discord")]
    essentials_discord::on_disable(plugin);
    #[cfg(feature = "xmpp")]
    essentials_xmpp::on_disable();
    essentials::on_disable(plugin);
}

bukkit_plugin!(on_enable, on_disable);
