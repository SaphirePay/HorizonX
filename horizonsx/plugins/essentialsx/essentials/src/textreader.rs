//! `textreader`: `IText`, `SimpleTextInput`, `TextInput`, `KeywordReplacer` and `TextPager`.
use crate::prelude::*;
use crate::player_list;
use crate::utils::format::strip_essentials_format;
use crate::utils::number::format_double;
use crate::utils::string::{capital_case, sanitize_file_name};
use regex::Regex;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::LazyLock;

/// `SimpleTextInput`.
#[derive(Clone, Debug, Default)]
pub struct SimpleTextInput {
    pub lines: Vec<String>,
    pub chapters: Vec<String>,
    pub bookmarks: BTreeMap<String, usize>,
}

impl SimpleTextInput {
    pub fn new(input: &str) -> Self {
        Self::from_lines(input.split('\n').map(str::to_owned).collect())
    }
    pub fn from_lines(lines: Vec<String>) -> Self {
        Self { lines, ..Self::default() }
    }
    pub fn add_line(&mut self, line: String) {
        self.lines.push(line);
    }
    pub fn get_lines(&self) -> &[String] {
        &self.lines
    }
}

fn resource_text(name: &str) -> Option<&'static str> {
    Some(match name {
        "motd" => include_str!("../resources/motd.txt"),
        "rules" => include_str!("../resources/rules.txt"),
        "info" => include_str!("../resources/info.txt"),
        "book" => include_str!("../resources/book.txt"),
        "custom" => include_str!("../resources/custom.txt"),
        _ => return None,
    })
}

/// `new TextInput(sender, filename, createFile, ess)`: `Err` is Java's `IOException` for a file just made.
pub fn text_input(sender: &CommandSource, filename: &str, create_file: bool) -> Result<SimpleTextInput, String> {
    let folder = crate::data_folder();
    let mut file: Option<PathBuf> = None;
    if let Some(user) = sender.get_user() {
        let by_name = folder.join(format!("{filename}_{}.txt", sanitize_file_name(&user.get_name())));
        file = Some(if by_name.exists() { by_name } else { folder.join(format!("{filename}_{}.txt", sanitize_file_name(&user.get_group()))) });
    }
    let file = file.filter(|file| file.exists()).unwrap_or_else(|| folder.join(format!("{filename}.txt")));
    let Ok(text) = std::fs::read_to_string(&file) else {
        if create_file {
            let _ = std::fs::write(&file, resource_text(filename).unwrap_or_default());
            return Err(format!("File {filename}.txt does not exist. Creating one for you."));
        }
        return Ok(SimpleTextInput::default());
    };
    let mut input = SimpleTextInput::default();
    for (number, line) in text.lines().enumerate() {
        if line.len() > 1 && line.starts_with('#') {
            let titles: Vec<String> = line[1..].trim().replace(' ', "_").split(',').map(str::to_owned).collect();
            input.chapters.push(replace_format(&titles[0]));
            for title in &titles {
                input.bookmarks.insert(strip_essentials_format(&title.to_lowercase()), number);
            }
        }
        input.lines.push(replace_format(line));
    }
    Ok(input)
}

static KEYWORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{([^\{\}]+)\}").expect("a valid pattern"));

/// `KeywordType.isPrivate`, for those that are.
fn is_private(keyword: &str) -> bool {
    matches!(keyword, "PLAYERLIST" | "IP" | "ADDRESS" | "PLUGINS" | "VERSION")
}

fn replacement(keyword: &str, tokens: &[&str], user: Option<&User>, list: &mut Option<HashMap<String, String>>) -> Option<String> {
    Some(match keyword {
        "PLAYER" | "DISPLAYNAME" => user.map(User::get_display_name).unwrap_or_default(),
        "USERNAME" => user.map(User::get_name).unwrap_or_default(),
        "NICKNAME" => user.map(|user| user.get_formatted_nickname().unwrap_or_else(|| user.get_name())).unwrap_or_default(),
        "PREFIX" => user.and_then(User::player).map(|player| replace_format(&crate::perm::get_prefix(&player))).unwrap_or_default(),
        "SUFFIX" => user.and_then(User::player).map(|player| replace_format(&crate::perm::get_suffix(&player))).unwrap_or_default(),
        "GROUP" => user.map(User::get_group).unwrap_or_default(),
        "BALANCE" => user.map(|user| crate::adventure::mini_to_legacy(&display_currency(&user.get_money()))).unwrap_or_default(),
        "MAILS" => user.map(|user| user.get_mail_amount().to_string()).unwrap_or_default(),
        "PLAYTIME" => user.and_then(User::player).map(|player| format_date_diff(crate::now_ms() - crate::ess::play_one_tick(&player) * 50)).unwrap_or_default(),
        "WORLD" | "WORLDNAME" => user.and_then(User::get_world).map(|world| world.get_name().to_owned()).unwrap_or_default(),
        "ONLINE" => {
            let users = ess().get_online_users();
            (users.len() - users.iter().filter(|user| user.is_hidden()).count()).to_string()
        }
        "UNIQUE" => crate::utils::number::DecimalFormat::new("#,##0", crate::big_decimal::Rounding::HalfEven).format(&crate::big_decimal::BigDecimal::from_i64(ess().users().get_user_count() as i64)),
        "WORLDS" => horizon_bukkit::get_worlds().iter().map(|world| world.get_name().to_owned()).collect::<Vec<_>>().join(", "),
        "PLAYERLIST" => {
            let outputs = list.get_or_insert_with(|| {
                let show_hidden = user.is_none_or(|user| user.is_authorized("essentials.list.hidden") || user.can_interact_vanished());
                let mut map = HashMap::new();
                for (group, users) in player_list::get_player_lists(user, show_hidden) {
                    if !users.is_empty() {
                        map.insert(group, crate::adventure::mini_to_legacy(&player_list::list_users(users, " ")));
                    }
                }
                let names: Vec<String> = ess().get_online_users().iter().filter(|user| !user.is_hidden()).map(User::get_display_name).collect();
                map.insert(String::new(), names.join(", "));
                map
            });
            match tokens.get(1) {
                None => outputs.get("").cloned().unwrap_or_default(),
                Some(group) => outputs.get(&group.to_lowercase()).cloned().or_else(|| tokens.get(2).map(|text| (*text).to_owned())).unwrap_or_default(),
            }
        }
        "TIME" => chrono::Local::now().format("%-I:%M:%S %p").to_string(),
        "DATE" => chrono::Local::now().format("%b %-d, %Y").to_string(),
        "COORDS" => user.and_then(User::get_location).zip(user).map(|(at, user)| user.player_tl("coordsKeyword", crate::args![at.get_block_x(), at.get_block_y(), at.get_block_z()])).unwrap_or_default(),
        "TPS" => format_double(ess().timer_average_tps()),
        "UPTIME" => format_date_diff(ess().start_time()),
        "IP" | "ADDRESS" => user.and_then(User::player).and_then(|player| player.get_address()).unwrap_or_default(),
        "PLUGINS" => ess().plugin().get_name().to_owned(),
        "VERSION" => horizon_bukkit::get_version(),
        "WORLDTIME12" => user.and_then(User::get_world).map(|world| crate::utils::tick_format::format12(world.get_time())).unwrap_or_default(),
        "WORLDTIME24" => user.and_then(User::get_world).map(|world| crate::utils::tick_format::format24(world.get_time())).unwrap_or_default(),
        // `DateFormat.MEDIUM` of a calendar that starts in year 0: nobody has found a use for this one.
        "WORLDDATE" => "N/A".to_owned(),
        _ => return None,
    })
}

/// `KeywordReplacer`: the result's lines, with `{PLAYER}` and such filled in.
pub struct KeywordReplacer {
    lines: Vec<String>,
}

impl KeywordReplacer {
    pub fn new(input: SimpleTextInput, sender: &CommandSource, show_private: bool, spaces_to_underscores: bool) -> Self {
        let user = sender.get_user();
        let mut list = None;
        let mut lines = Vec::with_capacity(input.lines.len());
        for line in input.lines {
            if line.starts_with('@') {
                lines.push(line);
                continue;
            }
            let replaced = KEYWORD.replace_all(&line, |found: &regex::Captures<'_>| {
                let whole = &found[0];
                let tokens: Vec<&str> = found[1].split(':').collect();
                let keyword = tokens[0];
                if is_private(keyword) && !show_private {
                    return String::new();
                }
                match replacement(keyword, &tokens, user.as_ref(), &mut list) {
                    None => whole.to_owned(),
                    Some(text) if spaces_to_underscores && keyword != "USERNAME" && !line.starts_with('/') => text.replace('_', "\\_").replace(' ', "_"),
                    Some(text) => text,
                }
            });
            lines.push(replaced.into_owned());
        }
        Self { lines }
    }
    pub fn get_lines(&self) -> &[String] {
        &self.lines
    }
}

/// `SimpleTextPager.showPage` over a `SimpleTranslatableText`: nine lines to a page.
pub fn show_translatable_page(sender: &CommandSource, lines: &[(String, Vec<crate::i18n::Arg>)], page_str: Option<&str>, command_name: &str) {
    let page = page_str.and_then(|text| text.parse::<usize>().ok()).unwrap_or(1).max(1);
    let end = lines.len();
    let pages = end / 9 + usize::from(end % 9 > 0);
    if page > pages {
        sender.send_tl("infoUnknownChapter", &[]);
        return;
    }
    let content = match command_name.split_once(' ') {
        Some((first, rest)) => format!("{}: {rest}", capital_case(first)),
        None => capital_case(command_name),
    };
    sender.send_tl("infoPages", crate::args![page, pages, content]);
    for (key, args) in &lines[(page - 1) * 9..end.min(page * 9)] {
        sender.send_tl(key, args);
    }
    if page < pages {
        sender.send_tl("readNextPage", crate::args![command_name, page + 1]);
    }
}

/// `TextPager`.
pub struct TextPager<'a> {
    pub lines: &'a [String],
    pub chapters: &'a [String],
    pub bookmarks: &'a BTreeMap<String, usize>,
    pub one_page: bool,
}

impl TextPager<'_> {
    /// `showPage`.
    pub fn show_page(&self, page_str: Option<&str>, chapter_page_str: Option<&str>, command_name: Option<&str>, sender: &CommandSource) {
        let (lines, one_page) = (self.lines, self.one_page);
        let numeric = page_str.is_none_or(|text| text.is_empty() || text.bytes().all(|b| b.is_ascii_digit()));
        if numeric {
            if lines.first().is_some_and(|line| line.starts_with('#')) {
                if one_page {
                    return;
                }
                sender.send_tl("infoChapter", &[]);
                sender.send_message(&self.chapters.join(", "));
                return;
            }
            let page = page_str.and_then(|text| text.parse::<i32>().ok()).unwrap_or(1).max(1) as usize;
            let start = if one_page { 0 } else { (page - 1) * 9 };
            let end = lines.iter().position(|line| line.starts_with('#')).unwrap_or(lines.len());
            let pages = end.div_ceil(9);
            if page > pages {
                sender.send_tl("infoUnknownChapter", &[]);
                return;
            }
            if let Some(name) = command_name.filter(|_| !one_page) {
                let content = match name.split_once(' ') {
                    Some((first, rest)) => format!("{}: {rest}", capital_case(first)),
                    None => capital_case(name),
                };
                sender.send_tl("infoPages", crate::args![page, pages, content]);
            }
            for line in lines.iter().take(end).skip(start).take(if one_page { 20 } else { 9 }) {
                sender.send_message(&format!("\u{a7}r{line}"));
            }
            if let Some(name) = command_name.filter(|_| !one_page && page < pages) {
                sender.send_tl("readNextPage", crate::args![name, page + 1]);
            }
            return;
        }
        let page_str = page_str.unwrap_or_default();
        let chapter_page = chapter_page_str.and_then(|text| text.parse::<i32>().ok()).map_or(0, |page| (page - 1).max(0)) as usize;
        let Some(bookmark) = self.bookmarks.get(&page_str.to_lowercase()) else {
            sender.send_tl("infoUnknownChapter", &[]);
            return;
        };
        let chapter_start = bookmark + 1;
        let chapter_end = lines.iter().enumerate().skip(chapter_start).find(|(_, line)| line.starts_with('#')).map_or(lines.len(), |(at, _)| at);
        let start = chapter_start + if one_page { 0 } else { chapter_page * 9 };
        let page = chapter_page + 1;
        let pages = (chapter_end - chapter_start).div_ceil(9);
        if let Some(name) = command_name.filter(|_| !one_page) {
            sender.send_tl("infoChapterPages", crate::args![format!("{}: {page_str}", capital_case(name)), page, pages]);
        }
        for line in lines.iter().take(chapter_end).skip(start).take(if one_page { 20 } else { 9 }) {
            sender.send_message(&format!("\u{a7}r{line}"));
        }
        if let Some(name) = command_name.filter(|_| !one_page && page < pages) {
            sender.send_tl("readNextPage", crate::args![name, format!("{page_str} {}", page + 1)]);
        }
    }
}

/// `BookInput`: as `TextInput`, with no per-player file.
pub fn book_input(filename: &str, create_file: bool) -> Result<SimpleTextInput, String> {
    text_input(&CommandSource::console(), filename, create_file)
}

const PAGE_MAX: f64 = 254.0;
const CHAR_MAX: f64 = 18.5;
const LINE_MAX: usize = 12;

fn build_page(pages: &mut Vec<String>, lines: &mut Vec<String>, force: bool) -> bool {
    if force || lines.len() > LINE_MAX {
        pages.push(lines.iter().map(|line| format!("{line}\n")).collect());
        lines.clear();
        return true;
    }
    false
}

/// `BookPager.getPages`: a chapter of the book file cut into pages by the font's letter widths.
pub fn book_pages(text: &SimpleTextInput, page_str: &str) -> crate::error::EssResult<Vec<String>> {
    let lines = &text.lines;
    let bookmark = text.bookmarks.get(&page_str.to_lowercase()).ok_or_else(|| crate::error::EssError::tl("infoUnknownChapter", &[]))?;
    let chapter_start = bookmark + 1;
    let chapter_end = lines.iter().enumerate().skip(chapter_start).find(|(_, line)| line.starts_with('#')).map_or(lines.len(), |(at, _)| at);
    let mut pages = Vec::new();
    let mut page_lines = Vec::new();
    let mut page_length = 0.0;
    for page_line in &lines[chapter_start..chapter_end] {
        let chars: Vec<char> = page_line.chars().collect();
        let (mut length, mut pointer, mut start, mut weight, mut force_page_end) = (0.0, 0, 0, 1.0, false);
        while pointer < chars.len() {
            let letter = chars[pointer];
            if chars[start] == ' ' {
                start += 1;
                pointer += 1;
                continue;
            }
            if page_length >= PAGE_MAX {
                length = CHAR_MAX;
                page_length = 0.0;
                force_page_end = true;
            }
            if length >= CHAR_MAX || (letter == '\u{a7}' && length + 1.0 >= CHAR_MAX) {
                let (mut position, mut rollback) = (pointer, 0.0);
                while position > start && chars[position] != ' ' && chars[position] != '\n' {
                    rollback += 1.0;
                    position -= 1;
                }
                if position != start {
                    pointer = position;
                    page_length -= rollback;
                }
                page_lines.push(chars[start..pointer].iter().collect::<String>());
                if build_page(&mut pages, &mut page_lines, force_page_end) {
                    page_length = 0.0;
                }
                force_page_end = false;
                start = pointer;
                length = 0.0;
                page_length += 1.0;
            }
            page_length += 1.0;
            if letter == '\u{a7}' && pointer + 1 < chars.len() {
                weight = if matches!(chars[pointer + 1], 'l' | 'L') { 1.25 } else { 1.0 };
                pointer += 1;
            } else if matches!(letter, 'i' | '.' | ',' | '!' | ':' | ';' | '|') {
                length += 0.34 * weight;
            } else if matches!(letter, 'l' | '\'' | '`') {
                length += 0.53 * weight;
            } else if matches!(letter, ' ' | 't' | 'I' | '[' | ']') {
                length += 0.69 * weight;
            } else if matches!(letter, 'f' | 'k' | '"' | '*' | '(' | ')' | '{' | '}' | '<' | '>') {
                length += 0.85 * weight;
            } else if matches!(letter, '@' | '~') {
                length += 1.2 * weight;
            } else {
                length += weight;
            }
            pointer += 1;
        }
        if length > 0.0 {
            page_lines.push(chars[start..].iter().collect::<String>());
            if build_page(&mut pages, &mut page_lines, false) {
                page_length = 0.0;
            }
        }
    }
    build_page(&mut pages, &mut page_lines, true);
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chapter_cuts_into_lines_of_the_book() {
        let mut text = SimpleTextInput::from_lines(vec!["#one".into(), "a short line".into(), "#two".into(), "x".into()]);
        text.bookmarks.insert("one".into(), 0);
        assert_eq!(book_pages(&text, "one").unwrap(), ["a short line\n"]);
        assert!(book_pages(&text, "three").is_err());
    }
}
