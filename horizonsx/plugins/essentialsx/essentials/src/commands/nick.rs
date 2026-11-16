//! `Commandnick`. `NickChangeEvent` isn't fired: nothing could listen.
use crate::prelude::*;
use crate::config_entities::JavaPattern;
use crate::utils::format::unformat_string;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_source: Some(tab), ..Command::new("nick") };

fn first_word(nick: String) -> String {
    nick.split(' ').next().unwrap_or_default().to_owned()
}

fn run_user(user: &User, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    let source = user.get_source();
    if args.len() > 1 && user.is_authorized("essentials.nick.others") {
        let nick = first_word(format_nickname(Some(user), &args[1])?);
        loop_offline_players(&source, false, true, &args[0], |target| update_player(&source, target, &nick))?;
        user.send_tl("nickChanged", &[]);
        Ok(())
    } else {
        update_player(&source, user, &first_word(format_nickname(Some(user), &args[0])?))
    }
}

fn run_source(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 2, EssError::not_enough());
    let nick = first_word(format_nickname(None, &args[1])?);
    loop_offline_players(sender, false, true, &args[0], |target| update_player(sender, target, &nick))?;
    sender.send_tl("nickChanged", &[]);
    Ok(())
}

fn set_nickname(target: &User, nick: Option<String>) {
    target.set_nickname(nick);
    target.set_display_nick();
}

fn nick_shown(target: &User, nick: &str) -> String {
    if settings().change_display_name() { target.get_display_name() } else { nick.to_owned() }
}

fn update_player(sender: &CommandSource, target: &User, nick: &str) -> EssResult {
    if nick.eq_ignore_ascii_case("off") {
        set_nickname(target, None);
        target.send_tl("nickNoMore", &[]);
    } else if target.get_name().eq_ignore_ascii_case(nick) {
        set_nickname(target, Some(nick.to_owned()));
        if !strip_format(&target.get_display_name()).eq_ignore_ascii_case(&target.get_name()) {
            target.send_tl("nickNoMore", &[]);
        }
        target.send_tl("nickSet", crate::args![nick_shown(target, nick)]);
    } else if nick_in_use(target, nick) {
        return Err(EssError::NotEnoughArguments(sender.tl("nickInUse", &[])));
    } else {
        set_nickname(target, Some(nick.to_owned()));
        target.send_tl("nickSet", crate::args![nick_shown(target, nick)]);
    }
    Ok(())
}

fn format_nickname(user: Option<&User>, nick: &str) -> EssResult<String> {
    let new_nick = match user {
        Some(user) => format_string(user, "essentials.nick", nick),
        None => replace_format(nick),
    };
    let stripped = strip_format(&new_nick);
    fail_if!(!JavaPattern::new(&settings().get_nick_regex()).matches(&new_nick) && user.is_some_and(|user| !user.is_authorized("essentials.nick.allowunsafe")), EssError::tl("nickNamesAlpha", &[]));
    let length = if settings().ignore_colors_in_max_length() { stripped.encode_utf16().count() } else { unformat_string(&new_nick).encode_utf16().count() };
    fail_if!(length > settings().get_max_nick_length() as usize, EssError::tl("nickTooLong", &[]));
    fail_if!(stripped.is_empty(), EssError::tl("nickNamesAlpha", &[]));
    if let Some(user) = user {
        fail_if!(user.is_authorized("essentials.nick.changecolors") && !user.is_authorized("essentials.nick.changecolors.bypass") && stripped != user.get_name() && !nick.eq_ignore_ascii_case("off"), EssError::tl("nickNamesOnlyColorChanges", &[]));
        fail_if!(!user.is_authorized("essentials.nick.blacklist.bypass") && settings().get_nick_blacklist().iter().any(|pattern| pattern.find(&new_nick)), EssError::tl("nickNameBlacklist", crate::args![nick]));
    }
    Ok(new_nick)
}

fn nick_in_use(target: &User, nick: &str) -> bool {
    let lower = strip_format(&nick.to_lowercase());
    for online in ess().get_online_users() {
        if target.get_name() == online.get_name() {
            continue;
        }
        let taken = online.get_nickname().map(|known| strip_format(&known)).filter(|known| !known.is_empty());
        if taken.is_some_and(|known| known.to_lowercase() == lower) || lower == online.get_name().to_lowercase() {
            return true;
        }
    }
    ess().get_offline_user(&lower).is_some_and(|found| found != *target)
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 && sender.is_authorized("essentials.nick.others") { get_players(sender) } else { Vec::new() })
}
