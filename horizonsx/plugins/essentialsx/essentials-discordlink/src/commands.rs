//! `Commandlink`, `Commandunlink`, and the three commands on Discord's side.
use essentials::prelude::*;
use crate::link_manager;
use crate::manager::{AccountLinkManager, Cause};
use essentials_discord::api::{InteractionCommand, InteractionCommandArgument, InteractionCommandArgumentType, InteractionEvent, InteractionException};
use std::sync::Arc;

pub const LINK: Command = Command { run_user: Some(run_link), ..Command::new("link") };
pub const UNLINK: Command = Command { run_user: Some(run_unlink), ..Command::new("unlink") };

fn run_link(user: &User, _label: &str, _args: &[String]) -> EssResult {
    let Some(manager) = link_manager() else { return Ok(()) };
    if manager.is_uuid_linked(&user.get_uuid()) {
        user.send_tl("discordLinkLinkedAlready", &[]);
        return Ok(());
    }
    match manager.create_code(&user.get_uuid()) {
        Ok(code) => user.send_tl("discordLinkLinked", essentials::args![format!("/link {code}")]),
        Err(code) => user.send_tl("discordLinkPending", essentials::args![format!("/link {code}")]),
    }
    Ok(())
}

fn run_unlink(user: &User, _label: &str, _args: &[String]) -> EssResult {
    let Some(manager) = link_manager() else { return Ok(()) };
    if manager.remove_user(user, Cause::UnsyncPlayer) {
        user.send_tl("discordLinkUnlinked", &[]);
    } else {
        user.send_tl("discordLinkNoAccount", &[]);
    }
    Ok(())
}

/// One of the three: never disabled, always ephemeral.
struct LinkCommand {
    name: &'static str,
    description: String,
    arguments: Vec<InteractionCommandArgument>,
    accounts: Arc<AccountLinkManager>,
    run: fn(&AccountLinkManager, &Arc<AccountLinkManager>, InteractionEvent),
}

impl InteractionCommand for LinkCommand {
    fn is_disabled(&self) -> bool {
        false
    }
    fn is_ephemeral(&self) -> bool {
        true
    }
    fn get_name(&self) -> String {
        self.name.to_owned()
    }
    fn get_description(&self) -> String {
        self.description.clone()
    }
    fn get_arguments(&self) -> Vec<InteractionCommandArgument> {
        self.arguments.clone()
    }
    fn on_command(&self, event: InteractionEvent) {
        (self.run)(&self.accounts, &self.accounts, event);
    }
}

/// `AccountInteractionCommand.onCommand`.
fn account(accounts: &AccountLinkManager, _shared: &Arc<AccountLinkManager>, event: InteractionEvent) {
    let own = event.get_member().clone();
    let effective = event.get_user_argument("user").unwrap_or_else(|| own.clone());
    let is_self = own.get_id() == effective.get_id();
    match accounts.get_user(&effective.get_id()) {
        None if is_self => event.reply_tl("discordCommandAccountResponseNotLinked", essentials::args![effective.get_as_mention()]),
        None => event.reply_tl("discordCommandAccountResponseNotLinkedOther", essentials::args![effective.get_as_mention()]),
        Some(user) if is_self => event.reply_tl("discordCommandAccountResponseLinked", essentials::args![user.get_name()]),
        Some(user) => event.reply_tl("discordCommandAccountResponseLinkedOther", essentials::args![effective.get_as_mention(), user.get_name()]),
    }
}

/// `LinkInteractionCommand.onCommand`.
fn link(accounts: &AccountLinkManager, shared: &Arc<AccountLinkManager>, event: InteractionEvent) {
    if accounts.is_discord_linked(&event.get_member().get_id()) {
        event.reply_tl("discordCommandLinkHasAccount", &[]);
        return;
    }
    let Some(uuid) = event.get_string_argument("code").and_then(|code| accounts.get_pending_uuid(&code)) else {
        event.reply_tl("discordCommandLinkInvalidCode", &[]);
        return;
    };
    shared.register_account(&uuid, event.get_member(), Cause::SyncPlayer);
    event.reply_tl("discordCommandLinkLinked", &[]);
}

/// `UnlinkInteractionCommand.onCommand`.
fn unlink(_accounts: &AccountLinkManager, shared: &Arc<AccountLinkManager>, event: InteractionEvent) {
    if shared.remove_member(event.get_member(), Cause::UnsyncPlayer) {
        event.reply_tl("discordCommandUnlinkUnlinked", &[]);
    } else {
        event.reply_tl("discordCommandUnlinkInvalidCode", &[]);
    }
}

/// What `onEnable` registers.
pub fn register_interactions(accounts: &Arc<AccountLinkManager>) -> Result<(), InteractionException> {
    let Some(api) = essentials_discord::service() else { return Ok(()) };
    let controller = api.interaction_controller();
    if controller.get_command("link").is_some() {
        return Ok(());
    }
    let command = |name, description: &str, arguments, run| -> Arc<dyn InteractionCommand> { Arc::new(LinkCommand { name, description: tl_literal(description, &[]), arguments, accounts: accounts.clone(), run }) };
    let argument = |name, key: &str, kind, required| vec![InteractionCommandArgument::new(name, &tl_literal(key, &[]), kind, required)];
    controller.register_command(command("account", "discordCommandAccountDescription", argument("user", "discordCommandAccountArgumentUser", InteractionCommandArgumentType::User, false), account))?;
    controller.register_command(command("link", "discordCommandLinkDescription", argument("code", "discordCommandLinkArgumentCode", InteractionCommandArgumentType::String, true), link))?;
    controller.register_command(command("unlink", "discordCommandUnlinkDescription", Vec::new(), unlink))?;
    Ok(())
}
