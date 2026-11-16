//! `InteractionControllerImpl`, and `DiscordUtil.hasRoles`, which an `InteractionMember` uses.
use essentials::prelude::*;
use crate::api::{InteractionCommand, InteractionEvent, InteractionException, InteractionMember};
use crate::client::model::{Interaction, Role};
use crate::host::{self, Level};
use crate::service::JdaDiscordService;
use essentials::utils::string::abbreviate;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

type Commands = HashMap<String, Arc<dyn InteractionCommand>>;

/// `matchesRole`.
pub fn matches_role(role: &Role, definition: &str) -> bool {
    role.id.to_string() == definition || role.name.eq_ignore_ascii_case(definition)
}

/// `hasRoles`.
pub fn has_roles(admin: bool, member_id: u64, roles: &[Role], role_definitions: &[String]) -> bool {
    if admin {
        return true;
    }
    role_definitions.iter().map(|definition| definition.trim()).any(|definition| definition == "*" || member_id.to_string() == definition || roles.iter().any(|role| matches_role(role, definition)))
}

/// `Commands.slash`, and its options.
fn command_data(command: &dyn InteractionCommand, abbreviated: bool) -> Value {
    let shorten = |text: String| if abbreviated { abbreviate(&text, 100) } else { text };
    let options: Vec<Value> = command
        .get_arguments()
        .into_iter()
        .map(|argument| json!({ "type": argument.kind.option_type(), "name": argument.name, "description": shorten(argument.description), "required": argument.required }))
        .collect();
    json!({ "type": 1, "name": command.get_name(), "description": shorten(command.get_description()), "options": options })
}

#[derive(Default)]
pub struct InteractionController {
    service: Mutex<Weak<JdaDiscordService>>,
    command_map: Mutex<Commands>,
    batch_registration_queue: Mutex<Commands>,
    initial_batch_registration: AtomicBool,
}

fn locked(map: &Mutex<Commands>) -> MutexGuard<'_, Commands> {
    map.lock().unwrap_or_else(|e| e.into_inner())
}

impl InteractionController {
    pub(crate) fn attach(&self, service: &Arc<JdaDiscordService>) {
        *self.service.lock().unwrap_or_else(|e| e.into_inner()) = Arc::downgrade(service);
    }
    fn service(&self) -> Option<Arc<JdaDiscordService>> {
        self.service.lock().unwrap_or_else(|e| e.into_inner()).upgrade()
    }
    /// `onSlashCommandInteraction`. On the gateway's thread: the defer goes out before anything else.
    pub(crate) fn on_slash_command(&self, interaction: Interaction) {
        let Some(service) = self.service() else { return };
        let Some(discord) = service.discord() else { return };
        let Some(command) = locked(&self.command_map).get(&interaction.name).cloned() else { return };
        let (Some(guild), Some(member)) = (interaction.guild_id.and_then(|id| discord.guild(id)), interaction.member.clone()) else {
            return;
        };
        if command.is_disabled() {
            let body = json!({ "type": 4, "data": { "content": tl_literal("discordErrorCommandDisabled", &[]), "flags": 64 } });
            let _ = discord.interaction_callback(&interaction, body);
            return;
        }
        let data = if command.is_ephemeral() { json!({ "flags": 64 }) } else { json!({}) };
        let deferred = discord.interaction_callback(&interaction, json!({ "type": 5, "data": data }));
        let member = InteractionMember::of(&discord, &guild, member);
        let event = InteractionEvent::new(discord, interaction, member.clone(), deferred, service.host().clone());
        let snowflakes = service.settings().get_command_snowflakes(&command.get_name());
        if snowflakes.is_some_and(|snowflakes| !member.has_roles(&snowflakes)) {
            event.reply_tl("noAccessCommand", &[]);
            return;
        }
        service.host().run_main(Box::new(move || command.on_command(event)));
    }
    /// `getCommand`.
    pub fn get_command(&self, name: &str) -> Option<Arc<dyn InteractionCommand>> {
        locked(&self.command_map).get(name).cloned()
    }
    /// `processBatchRegistration`.
    pub fn process_batch_registration(&self) {
        if self.initial_batch_registration.load(Ordering::SeqCst) || locked(&self.batch_registration_queue).is_empty() {
            return;
        }
        let Some(service) = self.service() else { return };
        let Some(discord) = service.discord() else { return };
        self.initial_batch_registration.store(true, Ordering::SeqCst);
        let list: Vec<Value> = locked(&self.batch_registration_queue).values().map(|command| command_data(command.as_ref(), true)).collect();
        let weak = Arc::downgrade(&service);
        discord.put_guild_commands(service.guild_id(), Value::Array(list)).then(move |answer| {
            let Some(service) = weak.upgrade() else { return };
            let controller = service.interaction_controller();
            let host = service.host();
            match answer {
                Ok(success) => {
                    for command in success.as_array().into_iter().flatten() {
                        let name = command["name"].as_str().unwrap_or_default();
                        if let Some(queued) = locked(&controller.batch_registration_queue).remove(name) {
                            locked(&controller.command_map).insert(name.to_owned(), queued);
                        }
                        if host.is_debug() {
                            host::log(host, Level::Info, &format!("Registered guild command {name} with id {}", command["id"].as_str().unwrap_or_default()));
                        }
                    }
                    let mut queue = locked(&controller.batch_registration_queue);
                    if !queue.is_empty() {
                        host::log(host, Level::Warning, &format!("{} Discord commands were lost during command registration!", queue.len()));
                        if host.is_debug() {
                            host::log(host, Level::Warning, &format!("Lost commands: {:?}", queue.keys().collect::<Vec<_>>()));
                        }
                        queue.clear();
                    }
                }
                Err(error) if error.is_missing_access() => host::log(host, Level::Severe, &tl_literal("discordErrorCommand", &[])),
                Err(error) => host::log(host, Level::Severe, &format!("Error while registering command: {error}")),
            }
        });
    }
    /// `registerCommand`.
    pub fn register_command(&self, command: Arc<dyn InteractionCommand>) -> Result<(), InteractionException> {
        fail_if!(command.is_disabled(), InteractionException("The given command has been disabled!".to_owned()));
        let name = command.get_name();
        fail_if!(locked(&self.command_map).contains_key(&name), InteractionException("A command with that name is already registered!".to_owned()));
        let Some(service) = self.service() else {
            return Err(InteractionException("Discord isn't connected".to_owned()));
        };
        if !self.initial_batch_registration.load(Ordering::SeqCst) {
            if service.host().is_debug() {
                host::log(service.host(), Level::Info, &format!("Marked guild command for batch registration: {name}"));
            }
            locked(&self.batch_registration_queue).insert(name, command);
            return Ok(());
        }
        let Some(discord) = service.discord() else {
            return Err(InteractionException("Discord isn't connected".to_owned()));
        };
        let data = command_data(command.as_ref(), false);
        let weak = Arc::downgrade(&service);
        discord.post_guild_command(service.guild_id(), data).then(move |answer| {
            let Some(service) = weak.upgrade() else { return };
            let host = service.host();
            match answer {
                Ok(success) => {
                    locked(&service.interaction_controller().command_map).insert(name.clone(), command);
                    if host.is_debug() {
                        host::log(host, Level::Info, &format!("Registered guild command {name} with id {}", success["id"].as_str().unwrap_or_default()));
                    }
                }
                Err(error) if error.is_missing_access() => host::log(host, Level::Severe, &tl_literal("discordErrorCommand", &[])),
                Err(error) => host::log(host, Level::Severe, &format!("Error while registering command: {error}")),
            }
        });
        Ok(())
    }
    /// `shutdown`: the guild's commands go, and with them what's kept here.
    pub(crate) fn shutdown(&self) {
        if let Some(service) = self.service()
            && let Some(discord) = service.discord()
            && service.guild_id() != 0
        {
            match discord.put_guild_commands(service.guild_id(), json!([])).wait(std::time::Duration::from_secs(5)) {
                Some(Ok(_)) => {}
                Some(Err(error)) => host::log(service.host(), Level::Severe, &format!("Error while deleting commands: {error}")),
                None => host::log(service.host(), Level::Severe, "Error while deleting commands: timed out"),
            }
        }
        locked(&self.command_map).clear();
        locked(&self.batch_registration_queue).clear();
        self.initial_batch_registration.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn role(id: u64, name: &str) -> Role {
        Role { id, name: name.to_owned(), position: 1, permissions: 0, color: 0, managed: false }
    }

    #[test]
    fn has_roles_takes_names_ids_and_stars() {
        let roles = [role(5, "Admins")];
        let list = |text: &str| vec![text.to_owned()];
        assert!(has_roles(false, 1, &roles, &list("admins")));
        assert!(has_roles(false, 1, &roles, &list(" 5 ")));
        assert!(has_roles(false, 1, &roles, &list("*")));
        assert!(has_roles(false, 1, &[], &list("1")));
        assert!(!has_roles(false, 1, &roles, &list("Mods")));
        assert!(has_roles(true, 1, &[], &[]));
    }
}
