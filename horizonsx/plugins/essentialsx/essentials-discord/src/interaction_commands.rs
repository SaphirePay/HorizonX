//! `ExecuteCommand`, `ListCommand` and `MessageCommand`, with `InteractionCommandImpl`'s part.
use essentials::prelude::*;
use crate::api::{InteractionCommand, InteractionCommandArgument, InteractionCommandArgumentType, InteractionEvent};
use crate::message_util::sanitize_discord_markdown;
use crate::recipient::DiscordMessageRecipient;
use crate::service::JdaDiscordService;
use essentials::messaging::{Recipient, Remote};
use essentials::player_list::{get_player_lists, list_group_users, list_summary, prepare_grouped_list};
use essentials::utils::format::strip_mini_format;
use std::sync::{Arc, Weak};

type Run = fn(&Arc<JdaDiscordService>, &[String], InteractionEvent);

/// `InteractionCommandImpl`.
struct BuiltIn {
    service: Weak<JdaDiscordService>,
    name: &'static str,
    description: String,
    arguments: Vec<InteractionCommandArgument>,
    run: Run,
}

impl InteractionCommand for BuiltIn {
    fn is_disabled(&self) -> bool {
        self.service.upgrade().is_none_or(|service| !service.settings().is_command_enabled(self.name))
    }
    fn is_ephemeral(&self) -> bool {
        self.service.upgrade().is_none_or(|service| service.settings().is_command_ephemeral(self.name))
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
        if let Some(service) = self.service.upgrade() {
            let admins = service.settings().get_command_admin_snowflakes(self.name);
            (self.run)(&service, &admins, event);
        }
    }
}

fn execute(service: &Arc<JdaDiscordService>, _admins: &[String], event: InteractionEvent) {
    let Some(command) = event.get_string_argument("command") else { return };
    event.reply_tl("discordCommandExecuteReply", essentials::args![command.clone()]);
    let host = service.host().clone();
    let target = host.clone();
    host.run_main(Box::new(move || target.dispatch_command(&command, Box::new(move |message| event.reply(&sanitize_discord_markdown(&message))))));
}

fn list(_service: &Arc<JdaDiscordService>, admins: &[String], event: InteractionEvent) {
    let show_hidden = event.get_member().has_roles(admins);
    let mut output = vec![list_summary(None, show_hidden)];
    let mut player_list = get_player_lists(None, show_hidden);
    match event.get_string_argument("group") {
        Some(group) => match list_group_users(&mut player_list, &group) {
            Ok(line) => output.push(line),
            Err(error) => output.push(tl_literal("errorWithMessage", essentials::args![error.get_message()])),
        },
        None => output.extend(prepare_grouped_list(None, "list", &mut player_list)),
    }
    let lines: Vec<String> = output.iter().map(|line| strip_mini_format(line)).collect();
    event.reply(&sanitize_discord_markdown(&lines.join("\n")));
}

fn message(service: &Arc<JdaDiscordService>, admins: &[String], event: InteractionEvent) {
    let get_hidden = event.get_member().has_roles(admins);
    let (Some(username), Some(text)) = (event.get_string_argument("username"), event.get_string_argument("message")) else {
        return;
    };
    let user = match ess().match_user(None, &username, get_hidden, false) {
        Ok(user) => user,
        Err(error) if error.is_player_not_found() => {
            event.reply_tl("errorWithMessage", essentials::args![error.get_message()]);
            return;
        }
        Err(_) => return,
    };
    let name = sanitize_discord_markdown(&user.get_display_name());
    if !get_hidden && user.is_ignore_msg() {
        event.reply_tl("msgIgnore", essentials::args![name]);
        return;
    }
    if user.is_afk() {
        match user.get_afk_message() {
            Some(afk) => event.reply_tl("userAFKWithMessage", essentials::args![name.clone(), sanitize_discord_markdown(&afk)]),
            None => event.reply_tl("userAFK", essentials::args![name.clone()]),
        }
    }
    let text = if event.get_member().has_roles(&service.settings().get_permitted_formatting_roles()) { replace_format(&text) } else { strip_format(&text) };
    event.reply_tl("msgFormat", essentials::args![tl_literal("meSender", &[]), name, sanitize_discord_markdown(&text)]);
    user.send_tl("msgFormat", &[event.get_member().get_name().into(), parsed(user.player_tl("meRecipient", &[])), text.into()]);
    // Java lets go of its own reference after five minutes, and the user's stays: this is the user's.
    let recipient = Recipient::Remote(Remote(Arc::new(DiscordMessageRecipient::new(event.get_member().clone()))));
    Recipient::User(user).set_reply_recipient(Some(recipient));
}

fn built_in(service: &Arc<JdaDiscordService>, name: &'static str, description: &str, arguments: Vec<InteractionCommandArgument>, run: Run) -> Arc<dyn InteractionCommand> {
    Arc::new(BuiltIn { service: Arc::downgrade(service), name, description: tl_literal(description, &[]), arguments, run })
}

/// The three of them, for `startup`: each is refused if it's turned off.
pub fn register(service: &Arc<JdaDiscordService>) {
    use InteractionCommandArgumentType::String as Text;
    let argument = |name, key: &str, required| InteractionCommandArgument::new(name, &tl_literal(key, &[]), Text, required);
    let commands = [
        built_in(service, "execute", "discordCommandExecuteDescription", vec![argument("command", "discordCommandExecuteArgumentCommand", true)], execute),
        built_in(service, "msg", "discordCommandMessageDescription", vec![argument("username", "discordCommandMessageArgumentUsername", true), argument("message", "discordCommandMessageArgumentMessage", true)], message),
        built_in(service, "list", "discordCommandListDescription", vec![argument("group", "discordCommandListArgumentGroup", false)], list),
    ];
    for command in commands {
        let _ = service.interaction_controller().register_command(command);
    }
}
