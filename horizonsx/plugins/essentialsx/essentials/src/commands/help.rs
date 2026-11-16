//! `Commandhelp`.
use crate::prelude::*;
use crate::help_input::help_lines;
use crate::i18n::has_key;
use crate::textreader::{KeywordReplacer, TextPager, text_input};
use horizon_bukkit::fam_core::get_command_map;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_source: Some(tab), ..Command::new("help") };

fn run_user(user: &User, label: &str, args: &[String]) -> EssResult {
    let source = user.get_source();
    let mut page = args.first().cloned();
    let mut chapter_page = args.get(1).cloned();
    let mut command = label.to_owned();
    let input = text_input(&source, "help", false).map_err(EssError::Message)?;
    let (chapters, bookmarks) = (input.chapters.clone(), input.bookmarks.clone());
    let lines = if input.lines.is_empty() {
        if let Some(cmd) = page.as_deref().and_then(|text| text.strip_prefix('/'))
            && show_command(user, cmd)
        {
            return Ok(());
        }
        match page.clone().filter(|text| !is_int(text)) {
            None => {
                chapter_page = None;
                help_lines(user, "")
            }
            Some(text) => {
                let text = if text.chars().count() > 26 { text.chars().take(25).collect() } else { text };
                let lines = help_lines(user, &text.to_lowercase());
                command = format!("{command} {text}");
                page = chapter_page.take();
                lines
            }
        }
    } else {
        user.set_display_nick();
        KeywordReplacer::new(input, &source, true, false).get_lines().to_vec()
    };
    TextPager { lines: &lines, chapters: &chapters, bookmarks: &bookmarks, one_page: false }.show_page(page.as_deref(), chapter_page.as_deref(), Some(&command), &source);
    Ok(())
}

/// The block for `/help /<command>`: `false` if nothing has that name.
fn show_command(user: &User, cmd: &str) -> bool {
    let known = get_command_map().get_known_commands();
    let Some(bukkit) = known.iter().find(|(word, _)| word.eq_ignore_ascii_case(cmd)).map(|(_, command)| command) else { return false };
    let name = bukkit.get_name();
    let is_ours = super::TABLE.iter().any(|command| command.name == name);
    user.send_tl("commandHelpLine1", crate::args![cmd]);
    let key = format!("{name}CommandDescription");
    let description = if is_ours && has_key(&key) { user.player_tl(&key, &[]) } else { bukkit.get_description().to_owned() };
    user.send_tl("commandHelpLine2", crate::args![description]);
    user.send_tl("commandHelpLine4", crate::args![format!("[{}]", bukkit.get_aliases().join(", "))]);
    user.send_tl("commandHelpLine3", &[]);
    let usages = if is_ours { usage_strings(name) } else { Vec::new() };
    if usages.is_empty() {
        user.send_message(bukkit.get_usage());
    }
    for (usage, description) in usages {
        user.send_tl("commandHelpLineUsage", &[parsed(usage.replace("<command>", cmd)), parsed(user.player_tl(&description, &[]))]);
    }
    true
}

fn run_source(sender: &CommandSource, _label: &str, _args: &[String]) -> EssResult {
    sender.send_tl("helpConsole", &[]);
    Ok(())
}

fn tab(_sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    if args.len() != 1 {
        return Some(Vec::new());
    }
    let mut suggestions = get_commands();
    suggestions.extend(get_plugins());
    Some(suggestions)
}
