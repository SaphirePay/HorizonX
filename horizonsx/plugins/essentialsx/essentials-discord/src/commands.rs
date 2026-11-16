//! `Commanddiscord` and `Commanddiscordbroadcast`.
use essentials::prelude::*;
use crate::emoji::parse_to_aliases;
use crate::message_util::sanitize_discord_markdown;
use crate::service;

pub const DISCORD: Command = Command { run_source: Some(run_discord), ..Command::new("discord") };
pub const DISCORD_BROADCAST: Command = Command { run_source: Some(run_broadcast), tab_source: Some(tab_broadcast), ..Command::new("discordbroadcast") };

fn run_discord(sender: &CommandSource, _label: &str, _args: &[String]) -> EssResult {
    if let Some(service) = service() {
        sender.send_tl("discordCommandLink", essentials::args![service.settings().get_discord_url()]);
    }
    Ok(())
}

fn run_broadcast(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 2, EssError::not_enough());
    fail_if!(!sender.is_authorized(&format!("essentials.discordbroadcast.{}", args[0])), EssError::tl("discordbroadcastPermission", essentials::args![args[0].clone()]));
    let mut message = get_final_arg(args, 1);
    if !sender.is_authorized("essentials.discordbroadcast.markdown") {
        message = sanitize_discord_markdown(&message);
    }
    let channel = service().filter(|service| service.is_live()).and_then(|service| Some((service.broadcast_channel(&args[0])?, service)));
    let Some(((channel, name, can_talk), service)) = channel else {
        return Err(EssError::tl("discordbroadcastInvalidChannel", essentials::args![args[0].clone()]));
    };
    fail_if!(!can_talk, EssError::tl("discordNoSendPermission", essentials::args![name]));
    service.broadcast(channel, &service.parse_message_emotes(&message), sender.is_authorized("essentials.discordbroadcast.ping"));
    sender.send_tl("discordbroadcastSent", essentials::args![format!("#{}", parse_to_aliases(&name))]);
    Ok(())
}

fn tab_broadcast(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    let service = service()?;
    if args.len() == 1 {
        let mut channels = service.settings().get_channel_names();
        channels.retain(|channel| sender.is_authorized(&format!("essentials.discordbroadcast.{channel}")));
        return Some(channels);
    }
    let current = args.last()?;
    if current.starts_with(':') && (current.len() == 1 || !current.ends_with(':')) {
        return Some(service.emoji_names().into_iter().map(|name| format!(":{name}:")).collect());
    }
    Some(Vec::new())
}
