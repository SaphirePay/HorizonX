//! `Commandmail`.
use crate::prelude::*;
use crate::mail::{MailSender, get_mail_tl_args, get_mail_tl_key, send_mail};
use crate::textreader::show_translatable_page;
use crate::utils::date::parse_date_diff;
use crate::utils::format::format_message;
use crate::utils::number::is_positive_int;
use crate::utils::string::sanitize_string;
use std::sync::Mutex;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_user: Some(tab_user), tab_source: Some(tab_source), ..Command::new("mail") };

/// `timestamp`, `mailsPerMinute`.
static RATE: Mutex<(i64, i32)> = Mutex::new((0, 0));

fn is(args: &[String], word: &str) -> bool {
    args.first().is_some_and(|first| first.eq_ignore_ascii_case(word))
}

fn require(user: &User, node: &str) -> EssResult {
    if user.is_authorized(node) { Ok(()) } else { Err(EssError::tl("noPerm", crate::args![node])) }
}

fn check_muted(user: &User) -> EssResult {
    if !user.is_muted() {
        return Ok(());
    }
    fail_if!(user.get_mute_timeout() > 0, EssError::tl(if user.has_mute_reason() { "voiceSilencedReasonTime" } else { "voiceSilencedTime" }, crate::args![format_date_diff(user.get_mute_timeout()), user.get_mute_reason()]));
    Err(EssError::tl(if user.has_mute_reason() { "voiceSilencedReason" } else { "voiceSilenced" }, crate::args![user.get_mute_reason()]))
}

fn recipient(name: &str) -> EssResult<User> {
    get_player_term_raw(name, true, true).map_err(|error| if error.is_player_not_found() { EssError::tl("playerNeverOnServer", crate::args![name]) } else { error })
}

fn message_of(user: &User, text: &str) -> EssResult<String> {
    let message = format_message(user, "essentials.mail", &sanitize_string(&strip_format(text)));
    fail_if!(message.encode_utf16().count() > 1000, EssError::tl("mailTooLong", &[]));
    Ok(message)
}

fn deliver(user: &User, to: &User, message: &str, expire: i64) -> EssResult {
    if to.is_ignored_player(user) {
        return Ok(());
    }
    let mut rate = RATE.lock().unwrap_or_else(|e| e.into_inner());
    if (now_ms() - rate.0).abs() > 60_000 {
        *rate = (now_ms(), 0);
    }
    rate.1 += 1;
    fail_if!(rate.1 > settings().get_mails_per_minute(), EssError::tl("mailDelay", crate::args![settings().get_mails_per_minute()]));
    drop(rate);
    send_mail(to, &MailSender::of_user(user), message, expire);
    Ok(())
}

fn send_all(sender: MailSender, message: String, expire: i64) {
    ess().run_task_asynchronously(move || {
        for uuid in ess().users().get_all_user_uuids() {
            if let Some(user) = ess().users().load_uncached_user(&uuid) {
                send_mail(&user, &sender, &message, expire);
            }
        }
    });
}

fn clear_all() {
    ess().run_task_asynchronously(|| {
        for uuid in ess().users().get_all_user_uuids() {
            if let Some(user) = ess().users().load_uncached_user(&uuid) {
                user.set_mail_list(Vec::new());
            }
        }
    });
}

/// The shared end of `/mail clear`: `to` hears how it went.
fn clear(to: &CommandSource, mail_user: &User, to_remove: i32, empty_key: &str) -> EssResult {
    let mut mails = mail_user.get_mail_messages();
    if mails.is_empty() {
        to.send_tl(empty_key, crate::args![mail_user.get_display_name()]);
        return Err(EssError::NoCharge);
    }
    if to_remove > 0 {
        if to_remove as usize > mails.len() {
            to.send_tl("mailClearIndex", crate::args![mails.len()]);
            return Err(EssError::NoCharge);
        }
        mails.remove(to_remove as usize - 1);
        mail_user.set_mail_list(mails);
    } else {
        mail_user.set_mail_list(Vec::new());
    }
    to.send_tl("mailCleared", &[]);
    Ok(())
}

fn read(user: &User, label: &str, args: &[String]) -> EssResult {
    let mail = user.get_mail_messages();
    if mail.is_empty() {
        user.send_tl("noMail", &[]);
        return Err(EssError::NoCharge);
    }
    let mut lines = Vec::new();
    let mut kept = Vec::new();
    for mut message in mail {
        if message.is_expired() {
            continue;
        }
        lines.push((get_mail_tl_key(&message), get_mail_tl_args(&message)));
        message.read = true;
        kept.push(message);
    }
    if lines.is_empty() {
        user.send_tl("noMail", &[]);
        return Err(EssError::NoCharge);
    }
    show_translatable_page(&user.get_source(), &lines, args.get(1).map(String::as_str), &format!("{label} {}", args[0]));
    user.send_tl("mailClear", &[]);
    user.set_mail_list(kept);
    Ok(())
}

fn run_user(user: &User, label: &str, args: &[String]) -> EssResult {
    if is(args, "read") {
        return read(user, label, args);
    }
    if args.len() >= 3 && is(args, "send") {
        require(user, "essentials.mail.send")?;
        check_muted(user)?;
        let to = recipient(&args[1])?;
        let message = message_of(user, &get_final_arg(args, 2))?;
        deliver(user, &to, &message, 0)?;
        user.send_tl("mailSentTo", crate::args![to.get_display_name(), to.get_name()]);
        user.send_message(&message);
        return Ok(());
    }
    if args.len() >= 4 && is(args, "sendtemp") {
        require(user, "essentials.mail.sendtemp")?;
        check_muted(user)?;
        let to = recipient(&args[1])?;
        let expire = parse_date_diff(&args[2], true)?;
        let message = message_of(user, &get_final_arg(args, 3))?;
        deliver(user, &to, &message, expire)?;
        user.send_tl("mailSentToExpire", crate::args![to.get_display_name(), format_date_diff(expire), to.get_name()]);
        user.send_message(&message);
        return Ok(());
    }
    if args.len() > 1 && is(args, "sendall") {
        require(user, "essentials.mail.sendall")?;
        send_all(MailSender::of_user(user), format_message(user, "essentials.mail", &sanitize_string(&strip_format(&get_final_arg(args, 1)))), 0);
        user.send_tl("mailSent", &[]);
        return Ok(());
    }
    if args.len() >= 3 && is(args, "sendtempall") {
        require(user, "essentials.mail.sendtempall")?;
        let message = format_message(user, "essentials.mail", &sanitize_string(&strip_format(&get_final_arg(args, 2))));
        send_all(MailSender::of_user(user), message, parse_date_diff(&args[1], true)?);
        user.send_tl("mailSent", &[]);
        return Ok(());
    }
    if is(args, "clear") {
        let mut mail_user = user.clone();
        let mut to_remove = -1;
        if let Some(second) = args.get(1) {
            if is_positive_int(second) {
                to_remove = second.parse().unwrap_or(-1);
            } else {
                require(user, "essentials.mail.clear.others")?;
                mail_user = get_player_for_offline(user, args, 1, true)?;
                if let Some(third) = args.get(2).filter(|third| is_positive_int(third)) {
                    to_remove = third.parse().unwrap_or(-1);
                }
            }
        }
        let empty_key = if mail_user == *user { "noMail" } else { "noMailOther" };
        return clear(&user.get_source(), &mail_user, to_remove, empty_key);
    }
    if is(args, "clearall") {
        require(user, "essentials.mail.clearall")?;
        clear_all();
        user.send_tl("mailClearedAll", &[]);
        return Ok(());
    }
    Err(EssError::not_enough())
}

fn run_source(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    let console = MailSender::console;
    if is(args, "read") {
        return Err(EssError::tl("onlyPlayers", crate::args![format!("{label} read")]));
    } else if args.len() > 1 && is(args, "clear") {
        let mail_user = get_player_term_raw(&args[1], true, true)?;
        let to_remove = args.get(2).filter(|third| is_positive_int(third)).and_then(|third| third.parse().ok()).unwrap_or(-1);
        return clear(sender, &mail_user, to_remove, "noMailOther");
    } else if is(args, "clearall") {
        clear_all();
        sender.send_tl("mailClearedAll", &[]);
        return Ok(());
    } else if args.len() >= 3 && is(args, "send") {
        send_mail(&recipient(&args[1])?, &console(), &replace_format(&get_final_arg(args, 2)), 0);
    } else if args.len() >= 4 && is(args, "sendtemp") {
        let to = recipient(&args[1])?;
        let expire = parse_date_diff(&args[2], true)?;
        send_mail(&to, &console(), &replace_format(&get_final_arg(args, 3)), expire);
    } else if args.len() >= 2 && is(args, "sendall") {
        send_all(console(), replace_format(&get_final_arg(args, 1)), 0);
    } else if args.len() >= 3 && is(args, "sendtempall") {
        let expire = parse_date_diff(&args[1], true)?;
        send_all(console(), replace_format(&get_final_arg(args, 2)), expire);
    } else if args.len() >= 2 {
        send_mail(&recipient(&args[0])?, &console(), &replace_format(&get_final_arg(args, 1)), 0);
    } else {
        return Err(EssError::not_enough());
    }
    sender.send_tl("mailSent", &[]);
    Ok(())
}

fn date_diffs() -> Vec<String> {
    COMMON_DATE_DIFFS.iter().map(|diff| (*diff).to_owned()).collect()
}

fn tab_user(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    let allowed = |word: &str| args[0].eq_ignore_ascii_case(word) && user.is_authorized(&format!("essentials.mail.{word}"));
    Some(match args.len() {
        1 => {
            let mut options = vec!["read".to_owned(), "clear".to_owned()];
            options.extend(["send", "sendtemp", "sendall", "sendtempall", "clearall"].iter().filter(|word| user.is_authorized(&format!("essentials.mail.{word}"))).map(|word| (*word).to_owned()));
            options
        }
        2 if allowed("send") || allowed("sendtemp") || (args[0].eq_ignore_ascii_case("clear") && user.is_authorized("essentials.mail.clear.others")) => get_players(&user.get_source()),
        2 if allowed("sendtempall") => date_diffs(),
        2 if args[0].eq_ignore_ascii_case("read") => {
            let count = user.get_mail_messages().len();
            let pages = count / 9 + usize::from(count % 9 > 0);
            if pages == 0 { vec!["0".to_owned()] } else { (1..=pages).map(|page| page.to_string()).collect() }
        }
        2 if args[0].eq_ignore_ascii_case("clear") => (1..=user.get_mail_messages().len().min(9)).map(|index| index.to_string()).collect(),
        3 if allowed("sendtemp") => date_diffs(),
        _ => Vec::new(),
    })
}

fn tab_source(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    let word = args.first().map(|first| first.to_lowercase()).unwrap_or_default();
    Some(match (args.len(), word.as_str()) {
        (1, _) => ["send", "sendall", "sendtemp", "sendtempall", "clearall", "clear"].map(str::to_owned).to_vec(),
        (2, "send" | "sendtemp" | "clear") => get_players(sender),
        (2, "sendtempall") | (3, "sendtemp") => date_diffs(),
        _ => Vec::new(),
    })
}
