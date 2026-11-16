//! `Commandcustomtext`.
use crate::prelude::*;
use crate::textreader::{KeywordReplacer, TextPager, text_input};

pub const COMMAND: Command = Command { run_source: Some(run), ..Command::new("customtext") };

fn run(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    if let Some(user) = sender.get_user() {
        user.set_display_nick();
    }
    let input = text_input(sender, "custom", true).map_err(EssError::Message)?;
    let (chapters, bookmarks) = (input.chapters.clone(), input.bookmarks.clone());
    let output = KeywordReplacer::new(input, sender, true, false);
    let (chapter, page) = if label.eq_ignore_ascii_case("customtext") && !args.is_empty() && !is_int(label) { (args[0].as_str(), args.get(1)) } else { (label, args.first()) };
    TextPager { lines: output.get_lines(), chapters: &chapters, bookmarks: &bookmarks, one_page: false }.show_page(Some(chapter), page.map(String::as_str), None, sender);
    Ok(())
}
