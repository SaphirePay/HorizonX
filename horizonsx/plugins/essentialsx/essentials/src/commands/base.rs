//! `EssentialsCommand`, `EssentialsLoopCommand`, `EssentialsToggleCommand`, and the dispatch that
//! `Essentials.onCommandEssentials` does. This is the API a command file stands on: see COMMANDS.md.
use crate::fail_if;
use crate::adventure::{deserialize_mini_message, mini_to_legacy};
use crate::alternative_commands;
use crate::big_decimal::BigDecimal;
use crate::command_source::CommandSource;
use crate::error::{EssError, EssResult};
use crate::ess::{ess, settings};
use crate::future::EssFuture;
use crate::i18n::{self, parsed, tl_literal};
use crate::trade::Trade;
use crate::user::User;
use crate::utils::format::strip_format;
use crate::utils::string::to_uuid;
use horizon_bukkit::{CommandSender, Plugin, World, world::match_player};

pub const COMMON_DURATIONS: [&str; 5] = ["1", "60", "600", "3600", "86400"];
pub const COMMON_DATE_DIFFS: [&str; 9] = ["1m", "15m", "1h", "3h", "12h", "1d", "1w", "1mo", "1y"];

/// `run(Server, User, commandLabel, args)`.
pub type RunUser = fn(user: &User, label: &str, args: &[String]) -> EssResult;
/// `run(Server, CommandSource, commandLabel, args)`.
pub type RunSource = fn(sender: &CommandSource, label: &str, args: &[String]) -> EssResult;
/// `getTabCompleteOptions(Server, User, ...)`: `None` lets the game list players.
pub type TabUser = fn(user: &User, label: &str, args: &[String]) -> Option<Vec<String>>;
/// `getTabCompleteOptions(Server, CommandSource, ...)`.
pub type TabSource = fn(sender: &CommandSource, label: &str, args: &[String]) -> Option<Vec<String>>;

/// One command: a `pub const COMMAND` in its file. What Java overrides, this names; what it leaves
/// out behaves as `EssentialsCommand`'s default does.
#[derive(Clone, Copy)]
pub struct Command {
    /// The name in `plugin.yml`, and what `Commandname` was.
    pub name: &'static str,
    pub run_user: Option<RunUser>,
    pub run_source: Option<RunSource>,
    pub tab_user: Option<TabUser>,
    pub tab_source: Option<TabSource>,
}

impl Command {
    /// A command with nothing set: fill in what it has, `..Command::new("name")`.
    pub const fn new(name: &'static str) -> Self {
        Self { name, run_user: None, run_source: None, tab_user: None, tab_source: None }
    }
    /// `run(Server, User, label, cmd, args)`: the command's cost is checked first and charged after.
    fn run_for_user(&self, user: &User, label: &str, args: &[String]) -> EssResult {
        let charge = Trade::of_command(self.name);
        charge.is_affordable_for(user)?;
        match self.run_user {
            Some(run) => run(user, label, args)?,
            None => self.run_for_source(&user.get_source(), label, args)?,
        }
        charge.charge(user)
    }
    /// `run(Server, CommandSource, label, cmd, args)`.
    fn run_for_source(&self, sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
        match self.run_source {
            Some(run) => run(sender, label, args),
            None => Err(EssError::tl("onlyPlayers", crate::args![label])),
        }
    }
    /// `tabComplete`, with `getTabCompleteOptions`' defaults and the starts-with filter.
    fn tab_complete(&self, user: Option<&User>, sender: &CommandSource, label: &str, args: &[String]) -> Option<Vec<String>> {
        let last = args.last()?;
        let options = match (user, self.tab_user, self.tab_source) {
            (Some(user), Some(tab), _) => tab(user, label, args),
            (_, _, Some(tab)) => tab(sender, label, args),
            _ => Some(get_players(sender)),
        }?;
        let lower = last.to_lowercase();
        Some(options.into_iter().filter(|option| option.to_lowercase().starts_with(&lower)).collect())
    }
}

/// `getFinalArg`.
pub fn get_final_arg(args: &[String], start: usize) -> String {
    args.get(start..).map(|rest| rest.join(" ")).unwrap_or_default()
}

/// `getPlayer(server, sender, args, pos)`: online, hidden ones only if the sender may see them.
pub fn get_player(sender: &CommandSource, args: &[String], pos: usize) -> EssResult<User> {
    get_player_or_offline(sender, args, pos, false)
}

/// `getPlayer(server, sender, args, pos, getOffline)`.
pub fn get_player_or_offline(sender: &CommandSource, args: &[String], pos: usize, get_offline: bool) -> EssResult<User> {
    match sender.get_user() {
        Some(user) => {
            let hidden = user.can_interact_vanished();
            get_player_raw(Some(&user), args, pos, hidden, get_offline)
        }
        None => get_player_raw(None, args, pos, true, get_offline),
    }
}

/// `getPlayer(server, sender, searchTerm)`.
pub fn get_player_by_term(sender: &CommandSource, term: &str) -> EssResult<User> {
    match sender.get_user() {
        Some(user) => ess().match_user(Some(&user), term, user.can_interact_vanished(), false),
        None => ess().match_user(None, term, true, false),
    }
}

/// `getPlayer(server, sourceUser, args, pos, getHidden, getOffline)`: raw access.
pub fn get_player_raw(source: Option<&User>, args: &[String], pos: usize, get_hidden: bool, get_offline: bool) -> EssResult<User> {
    let term = args.get(pos).ok_or_else(EssError::not_enough)?;
    fail_if!(term.is_empty(), EssError::player_not_found());
    ess().match_user(source, term, get_hidden, get_offline)
}

/// `getPlayer(server, searchTerm, getHidden, getOffline)`.
pub fn get_player_term_raw(term: &str, get_hidden: bool, get_offline: bool) -> EssResult<User> {
    ess().match_user(None, term, get_hidden, get_offline)
}

/// `getPlayers`: names for tab completion that this sender may see.
pub fn get_players(sender: &CommandSource) -> Vec<String> {
    let use_display = settings().change_tab_complete_name();
    ess().get_online_users().iter().filter(|user| ess().can_interact_with_source(Some(sender), user)).map(|user| if use_display { strip_format(&user.get_display_name()) } else { user.get_name() }).collect()
}

/// `EssentialsLoopCommand.getPlayers`.
pub fn get_players_with_wildcards(sender: &CommandSource) -> Vec<String> {
    let mut players = get_players(sender);
    players.push("**".to_owned());
    players.push("*".to_owned());
    players
}

/// `getItems`, of those the last word can still become.
pub fn get_items(args: &[String]) -> Vec<String> {
    ess().item_db().list_names(args.last().map_or("", String::as_str))
}

/// `getMatchingItems`.
pub fn get_matching_items(args: &[String]) -> Vec<String> {
    let mut items = vec!["hand".to_owned(), "inventory".to_owned(), "blocks".to_owned()];
    if !args.last().is_none_or(String::is_empty) {
        items.extend(get_items(args));
    }
    items
}

/// `getPlugins`: this server has this plugin, as far as the SDK tells.
pub fn get_plugins() -> Vec<String> {
    horizon_bukkit::lead2::get_plugins()
}

/// `/motd`, `/rules`, `/info`: a text file, its keywords filled in, a page of it.
pub fn show_text_file(sender: &CommandSource, file: &str, label: &str, args: &[String]) -> EssResult {
    if let Some(user) = sender.get_user() {
        user.set_display_nick();
    }
    let input = crate::textreader::text_input(sender, file, true).map_err(EssError::Message)?;
    let (chapters, bookmarks) = (input.chapters.clone(), input.bookmarks.clone());
    let output = crate::textreader::KeywordReplacer::new(input, sender, true, false);
    crate::textreader::TextPager { lines: output.get_lines(), chapters: &chapters, bookmarks: &bookmarks, one_page: false }.show_page(args.first().map(String::as_str), args.get(1).map(String::as_str), Some(label), sender);
    Ok(())
}

/// `getCommands`: every plugin's, by name, the game's left out as `PluginIdentifiableCommand` leaves it.
pub fn get_commands() -> Vec<String> {
    let known = horizon_bukkit::fam_core::get_command_map().get_known_commands();
    let names: std::collections::BTreeSet<String> = known.iter().filter(|(word, _)| word.split_once(':').is_some_and(|(space, _)| space != "minecraft")).map(|(_, command)| command.get_name().to_owned()).collect();
    names.into_iter().collect()
}

/// `getNewExceptionFuture`: a future whose failure is shown to the sender.
pub fn get_new_exception_future(sender: &CommandSource, label: &str) -> EssFuture {
    let future = EssFuture::new();
    let (sender, label) = (sender.clone(), label.to_owned());
    future.exceptionally(move |error| ess().show_error(&sender, error, &label));
    future
}

/// `tabCompleteCommand`: what `command` would offer for the words after `index`.
pub fn tab_complete_command(sender: &CommandSource, command: &str, args: &[String], index: usize) -> Vec<String> {
    let Some(entry) = super::TABLE.iter().find(|entry| entry.name.eq_ignore_ascii_case(command)) else { return Vec::new() };
    let words = &args[index.min(args.len())..];
    entry.tab_complete(sender.get_user().as_ref(), sender, command, words).unwrap_or_default()
}

/// `sender.isPlayer() ? getDisplayName() : Console.NAME`, then the same with `Console.displayName()`.
pub fn ban_names(sender: &CommandSource) -> (String, String) {
    if sender.is_player() {
        (sender.get_display_name(), sender.get_display_name())
    } else {
        ("Console".to_owned(), crate::command_source::console_display_name())
    }
}

/// `/banip`'s target: an address, or a player's last, or the words as given.
pub fn ip_of(args: &[String]) -> EssResult<String> {
    let address = if crate::utils::format::valid_ip(&args[0]) {
        args[0].clone()
    } else {
        match get_player_raw(None, args, 0, true, true) {
            Ok(player) => player.get_last_login_address(),
            Err(error) if error.is_player_not_found() => args[0].clone(),
            Err(error) => return Err(error),
        }
    };
    fail_if!(address.is_empty(), EssError::player_not_found());
    Ok(address)
}

/// Kicks every player at this address.
pub fn kick_address(address: &str, reason: &str) {
    for player in horizon_bukkit::get_online_players() {
        if player.get_address().is_some_and(|known| known.eq_ignore_ascii_case(address)) {
            player.kick_player(reason);
        }
    }
}

/// A reason as typed: `\\n` and `|` end a line, `&` colours.
pub fn reason_of(text: &str) -> String {
    crate::utils::format::replace_format(&text.replace("\\n", "\n").replace('|', "\n"))
}

/// `showError`.
pub fn show_error(sender: &CommandSource, error: &EssError, label: &str) {
    ess().show_error(sender, error, label);
}

// ---- EssentialsLoopCommand ----

pub fn skip_hidden(sender: &CommandSource) -> bool {
    sender.get_user().is_some_and(|user| !user.can_interact_vanished())
}

pub fn hidden_from(sender: &CommandSource, user: &User) -> bool {
    sender.get_player().is_some_and(|player| user.is_hidden_to(player) && user.is_hidden_from(player))
}

/// `loopOnlinePlayersConsumer`.
pub fn loop_online_players(sender: &CommandSource, multiple: bool, wildcards: bool, term: &str, mut consumer: impl FnMut(&User) -> EssResult) -> EssResult {
    fail_if!(term.is_empty(), EssError::player_not_found());
    if term == "@s" || term == "@p" {
        if let Some(user) = sender.get_user() {
            return consumer(&user);
        }
    }
    let skip = skip_hidden(sender);
    if wildcards && (term == "**" || term == "*") {
        for user in ess().get_online_users() {
            if !(skip && hidden_from(sender, &user)) {
                consumer(&user)?;
            }
        }
        return Ok(());
    }
    if !multiple {
        return consumer(&get_player_by_term(sender, term)?);
    }
    fail_if!(term.trim().chars().count() < 2, EssError::player_not_found());
    let mut found = false;
    let matched = match_player(term);
    if matched.is_empty() {
        let text = term.to_lowercase();
        for user in ess().get_online_users() {
            if !(skip && hidden_from(sender, &user)) && strip_format(&user.get_display_name()).to_lowercase().contains(&text) {
                found = true;
                consumer(&user)?;
            }
        }
    } else {
        for player in matched {
            if let Some(user) = ess().get_user(&player).filter(|user| !(skip && hidden_from(sender, user))) {
                found = true;
                consumer(&user)?;
            }
        }
    }
    if found { Ok(()) } else { Err(EssError::player_not_found()) }
}

/// `loopOfflinePlayersConsumer`.
pub fn loop_offline_players(sender: &CommandSource, multiple: bool, wildcards: bool, term: &str, mut consumer: impl FnMut(&User) -> EssResult) -> EssResult {
    fail_if!(term.is_empty(), EssError::player_not_found());
    if term == "@s" || term == "@p" {
        if let Some(user) = sender.get_user() {
            return consumer(&user);
        }
    }
    if let Some(uuid) = to_uuid(term) {
        return consumer(&ess().get_user_by_uuid(&uuid).ok_or_else(EssError::player_not_found)?);
    }
    if wildcards && term == "**" {
        for uuid in ess().users().get_all_user_uuids() {
            if let Some(user) = ess().users().load_uncached_user(&uuid) {
                consumer(&user)?;
            }
        }
        return Ok(());
    }
    if wildcards && term == "*" {
        let skip = skip_hidden(sender);
        for user in ess().get_online_users() {
            if !(skip && hidden_from(sender, &user)) {
                consumer(&user)?;
            }
        }
        return Ok(());
    }
    if multiple {
        fail_if!(term.trim().chars().count() < 3, EssError::player_not_found());
        let matched = match_player(term);
        if matched.is_empty() {
            return consumer(&get_player_term_raw(term, true, true)?);
        }
        for player in matched {
            if let Some(user) = ess().get_user(&player) {
                consumer(&user)?;
            }
        }
        return Ok(());
    }
    consumer(&get_player_term_raw(term, true, true)?)
}

// ---- EssentialsToggleCommand ----

/// `togglePlayer`: `None` toggles.
pub type TogglePlayer = fn(sender: &CommandSource, user: &User, enabled: Option<bool>) -> EssResult;

/// `matchToggleArgument`.
pub fn match_toggle_argument(arg: &str) -> Option<bool> {
    if arg.eq_ignore_ascii_case("on") || arg.starts_with("ena") || arg.eq_ignore_ascii_case("1") {
        Some(true)
    } else if arg.eq_ignore_ascii_case("off") || arg.starts_with("dis") || arg.eq_ignore_ascii_case("0") {
        Some(false)
    } else {
        None
    }
}

/// `handleToggleWithArgs`.
pub fn handle_toggle_with_args(user: &User, args: &[String], others: &str, toggle: TogglePlayer) -> EssResult {
    let source = user.get_source();
    if let [arg] = args {
        let wanted = match_toggle_argument(arg);
        if wanted.is_none() && user.is_authorized(others) {
            toggle_other_players(&source, args, toggle)
        } else {
            toggle(&source, user, wanted)
        }
    } else if args.len() == 2 && user.is_authorized(others) {
        toggle_other_players(&source, args, toggle)
    } else {
        toggle(&source, user, None)
    }
}

/// `toggleOtherPlayers`.
pub fn toggle_other_players(sender: &CommandSource, args: &[String], toggle: TogglePlayer) -> EssResult {
    let first = args.first().filter(|first| first.trim().chars().count() >= 2).ok_or_else(EssError::player_not_found)?;
    let skip = skip_hidden(sender);
    let mut found = false;
    for player in match_player(first) {
        let Some(user) = ess().get_user(&player) else { continue };
        if skip && hidden_from(sender, &user) {
            continue;
        }
        found = true;
        toggle(sender, &user, args.get(1).and_then(|arg| match_toggle_argument(arg)))?;
    }
    if found { Ok(()) } else { Err(EssError::player_not_found()) }
}

/// `getTabCompleteOptions` for a toggle, as a user.
pub fn toggle_tab_user(user: &User, args: &[String], others: &str) -> Option<Vec<String>> {
    let on_off = || vec!["enable".to_owned(), "disable".to_owned()];
    Some(match args.len() {
        1 if user.is_authorized(others) => get_players(&user.get_source()),
        1 => on_off(),
        2 if user.is_authorized(others) => on_off(),
        _ => Vec::new(),
    })
}

/// `getTabCompleteOptions` for a toggle, from the console.
pub fn toggle_tab_source(sender: &CommandSource, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => get_players(sender),
        2 => vec!["enable".to_owned(), "disable".to_owned()],
        _ => Vec::new(),
    })
}

// ---- Dispatch ----

/// `addUsageString`: `[optional]` and `<required>` words in their colors.
fn colored_usage(usage: &str) -> String {
    use regex::Regex;
    use std::sync::LazyLock;
    static ARGUMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([ :>])(([\[<])[A-Za-z |]+[>\]])").expect("a valid pattern"));
    ARGUMENT
        .replace_all(usage, |found: &regex::Captures<'_>| {
            let color = tl_literal(if &found[3] == "<" { "commandArgumentRequired" } else { "commandArgumentOptional" }, &[]);
            format!("{}{color}{}<reset>", &found[1], found[2].replace('|', &format!("{}|{color}", tl_literal("commandArgumentOr", &[]))))
        })
        .into_owned()
}

/// `getUsageStrings`: each `nameCommandUsageN` there is, with its description key.
pub fn usage_strings(name: &str) -> Vec<(String, String)> {
    (1..).map(|index| format!("{name}CommandUsage{index}")).take_while(|key| i18n::has_key(key)).map(|key| (colored_usage(&tl_literal(&key, &[])), format!("{key}Description"))).collect()
}

fn show_usage(command: &Command, sender: &CommandSource, label: &str, message: &str) {
    let usages = usage_strings(command.name);
    let description_key = format!("{}CommandDescription", command.name);
    if settings().is_verbose_command_usages() && !usages.is_empty() {
        sender.send_tl("commandHelpLine1", crate::args![label]);
        sender.send_tl("commandHelpLine2", crate::args![sender.tl(&description_key, &[])]);
        sender.send_tl("commandHelpLine3", &[]);
        for (usage, description) in usages {
            sender.send_tl("commandHelpLineUsage", &[parsed(usage.replace("<command>", label)), parsed(sender.tl(&description, &[]))]);
        }
    } else {
        let declared = horizon_bukkit::fam_core::get_command_map().get_known_commands().remove(&format!("essentials:{}", command.name));
        let (description, usage) = match declared.filter(|_| !i18n::has_key(&description_key)) {
            Some(declared) => (declared.get_description().to_owned(), declared.get_usage().to_owned()),
            None => (mini_to_legacy(&tl_literal(&description_key, &[])), mini_to_legacy(&tl_literal(&format!("{}CommandUsage", command.name), &[]))),
        };
        sender.send_message(&description);
        sender.send_message(&usage.replace("<command>", label));
    }
    if !message.is_empty() {
        sender.send_component(&deserialize_mini_message(message));
    }
}

/// `onCommandEssentials`.
fn dispatch(command: &Command, bukkit_sender: &CommandSender, label: &str, words: &[&str]) {
    let args: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
    let user = bukkit_sender.as_player().and_then(|player| ess().get_user(player));
    if user.is_none() && settings().log_console_commands() {
        crate::info(&format!("{} issued server command: /{label} {}", bukkit_sender.get_name(), get_final_arg(&args, 0)));
    }
    if let Some(alternative) = alternative_commands::for_command(command.name, label) {
        alternative_commands::executed(label, &alternative);
        alternative.execute(bukkit_sender, label, words);
        return;
    }
    if settings().is_command_disabled(label) && alternative_commands::run_in_place_of_disabled(bukkit_sender, label, words) {
        return;
    }
    let sender = CommandSource::new(bukkit_sender.clone());
    if let Some(user) = &user
        && !settings().is_command_disabled("mail")
        && command.name != "mail"
        && user.is_authorized("essentials.mail")
    {
        user.notify_of_mail();
    }
    if label.eq_ignore_ascii_case("essversion") {
        sender.send_message(&format!("This server is running Essentials {}", ess().plugin().get_version()));
        return;
    }
    if settings().is_command_disabled(label) {
        sender.send_tl("commandDisabled", crate::args![label]);
        return;
    }
    if let Some(user) = &user {
        if !user.is_authorized_command(command.name, "essentials.") {
            crate::info(&mini_to_legacy(&tl_literal("deniedAccessCommand", crate::args![user.get_name()])));
            user.send_tl("noAccessCommand", &[]);
            return;
        }
        if user.is_jailed() && !user.is_authorized_command(command.name, "essentials.jail.allow.") {
            if user.get_jail_timeout() > 0 {
                user.send_tl("playerJailedFor", crate::args![user.get_name(), user.get_formatted_jail_time()]);
            } else {
                user.send_tl("jailMessage", &[]);
            }
            return;
        }
    }
    let result = match &user {
        Some(user) => command.run_for_user(user, label, &args),
        None => command.run_for_source(&sender, label, &args),
    };
    match result {
        Ok(()) | Err(EssError::NoCharge | EssError::QuietAbort) => {}
        Err(EssError::NotEnoughArguments(message)) => show_usage(command, &sender, label, &message),
        Err(error) => show_error(&sender, &error, label),
    }
}

/// `onTabCompleteEssentials`.
fn complete(command: &Command, bukkit_sender: &CommandSender, label: &str, words: &[&str]) -> Vec<String> {
    // Java takes the alternative's completer if it has one. Nothing back here may mean it hasn't, and
    // then Essentials' own list stands, as in Java.
    if let Some(alternative) = alternative_commands::for_command(command.name, label) {
        let list = alternative.tab_complete(bukkit_sender, label, words);
        if !list.is_empty() {
            return list;
        }
    }
    let args: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
    let user = bukkit_sender.as_player().and_then(|player| ess().get_user(player));
    let sender = CommandSource::new(bukkit_sender.clone());
    if settings().is_command_disabled(label) {
        return Vec::new();
    }
    if let Some(user) = &user
        && (!user.is_authorized_command(command.name, "essentials.") || (user.is_jailed() && !user.is_authorized_command(command.name, "essentials.jail.allow.")))
    {
        return Vec::new();
    }
    command.tab_complete(user.as_ref(), &sender, label, &args).unwrap_or_default()
}

/// What `reload` does for commands, once: every file in the table gets the plugin's executor.
pub fn register(plugin: Plugin) {
    register_commands(plugin, super::TABLE);
}

/// What `onCommandEssentials` does for a module: its commands, through the same door.
pub fn register_commands(plugin: Plugin, table: &'static [Command]) {
    for command in table.iter().copied() {
        plugin.set_executor(command.name, move |sender, label, words| {
            dispatch(&command, sender, label, words);
            true
        });
        plugin.set_tab_completer(command.name, move |sender, label, words| complete(&command, sender, label, words));
    }
}

/// What `/msg`, `/r`, `/me` and `/afk` throw for someone muted.
pub fn muted_error(user: &User) -> EssError {
    let date = (user.get_mute_timeout() > 0).then(|| crate::utils::date::format_date_diff(user.get_mute_timeout()));
    match (date, user.get_mute_reason()) {
        (None, Some(reason)) => EssError::tl("voiceSilencedReason", crate::args![reason]),
        (None, None) => EssError::tl("voiceSilenced", &[]),
        (Some(date), Some(reason)) => EssError::tl("voiceSilencedReasonTime", crate::args![date, reason]),
        (Some(date), None) => EssError::tl("voiceSilencedTime", crate::args![date]),
    }
}

/// `CommonPlaceholders.enableDisable`.
pub fn enable_disable(source: &CommandSource, enable: bool) -> crate::i18n::Arg {
    parsed(source.tl(if enable { "enabled" } else { "disabled" }, &[]))
}

/// `CommonPlaceholders.trueFalse`.
pub fn true_false(source: &CommandSource, condition: bool) -> crate::i18n::Arg {
    parsed(source.tl(if condition { "true" } else { "false" }, &[]))
}

/// `FloatUtil.parseFloat`: a finite number or nothing.
pub fn parse_float(text: &str) -> Option<f32> {
    text.trim().parse::<f32>().ok().filter(|value| value.is_finite())
}

/// True if this sender is that player: `sender.isPlayer() && sender.getPlayer().equals(user.getBase())`.
pub fn is_self(sender: &CommandSource, user: &User) -> bool {
    sender.get_player().is_some() && sender.get_player() == user.player().as_ref()
}

/// `getPlayer(server, user, args, pos)`.
pub fn get_player_for(user: &User, args: &[String], pos: usize) -> EssResult<User> {
    get_player_raw(Some(user), args, pos, user.can_interact_vanished(), false)
}

/// `getPlayer(server, user, args, pos, getOffline)`.
pub fn get_player_for_offline(user: &User, args: &[String], pos: usize, get_offline: bool) -> EssResult<User> {
    get_player_raw(Some(user), args, pos, user.can_interact_vanished(), get_offline)
}

/// The `getTabCompleteOptions` half of the commands: players for the first word, then nothing.
pub fn tab_player_first(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 { get_players(sender) } else { Vec::new() })
}

/// `getTabCompleteOptions` giving `Collections.emptyList()`.
pub fn tab_nothing(_sender: &CommandSource, _label: &str, _args: &[String]) -> Option<Vec<String>> {
    Some(Vec::new())
}

/// `user.getWorld() != w && isWorldTeleportPermissions() && !user.isAuthorized("essentials.worlds." + w)`.
pub fn check_world_permission(user: &User, differs: bool, world: &World) -> EssResult {
    let node = format!("essentials.worlds.{}", world.get_name());
    fail_if!(differs && settings().is_world_teleport_permissions() && !user.is_authorized(&node), EssError::tl("noPerm", crate::args![node]));
    Ok(())
}

/// `Double.parseDouble`: the message is Java's `NumberFormatException`.
pub fn parse_double(text: &str) -> EssResult<f64> {
    text.trim().parse().map_err(|_| EssError::Message(format!("For input string: \"{text}\"")))
}

/// `Integer.parseInt`.
pub fn parse_int(text: &str) -> EssResult<i32> {
    text.parse().map_err(|_| EssError::Message(format!("For input string: \"{text}\"")))
}

/// `~` and `~5` from a base, or a plain number.
pub fn coordinate(arg: &str, base: f64) -> EssResult<f64> {
    match arg.strip_prefix('~') {
        Some("") => Ok(base),
        Some(rest) => Ok(base + parse_double(rest)?),
        None => parse_double(arg),
    }
}

/// `x > 30000000 || ...`: the world's edge in any direction.
pub fn check_position(x: f64, y: f64, z: f64, invalid: impl FnOnce() -> String) -> EssResult {
    fail_if!([x, y, z].iter().any(|value| !(-30_000_000.0..=30_000_000.0).contains(value)), EssError::NotEnoughArguments(invalid()));
    Ok(())
}

/// Shorthand for commands: `new BigDecimal` from a text the code wrote.
pub fn decimal(text: &str) -> BigDecimal {
    BigDecimal::parse(text).unwrap_or_else(BigDecimal::zero)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_words() {
        assert_eq!(match_toggle_argument("on"), Some(true));
        assert_eq!(match_toggle_argument("enable"), Some(true));
        assert_eq!(match_toggle_argument("disable"), Some(false));
        assert_eq!(match_toggle_argument("Bob"), None);
    }

    #[test]
    fn final_arg_joins_the_rest() {
        let args: Vec<String> = ["a", "b", "c"].iter().map(|word| (*word).to_owned()).collect();
        assert_eq!(get_final_arg(&args, 1), "b c");
        assert_eq!(get_final_arg(&args, 3), "");
        assert_eq!(get_final_arg(&args, 4), "");
    }
}
