//! `EmojiParser.parseToAliases(text, REMOVE)`: emoji-java's job, on the `emojis` crate's table.

/// The longest emoji, in characters, that's worth looking for.
const LONGEST: usize = 12;

fn is_skin_tone(c: char) -> bool {
    ('\u{1F3FB}'..='\u{1F3FF}').contains(&c)
}

/// Emoji as `:alias:`, with any skin tone taken off. An emoji with no alias stays as it is.
pub fn parse_to_aliases(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index].is_ascii() {
            out.push(chars[index]);
            index += 1;
            continue;
        }
        let found = (1..=LONGEST.min(chars.len() - index)).rev().find_map(|length| {
            let candidate: String = chars[index..index + length].iter().filter(|c| !is_skin_tone(**c)).collect();
            emojis::get(&candidate).and_then(emojis::Emoji::shortcode).map(|alias| (alias, length))
        });
        match found {
            Some((alias, length)) => {
                out.push(':');
                out.push_str(alias);
                out.push(':');
                index += length;
            }
            None if is_skin_tone(chars[index]) && index > 0 => index += 1,
            None => {
                out.push(chars[index]);
                index += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emoji_become_aliases() {
        assert_eq!(parse_to_aliases("hi \u{1F604} there"), "hi :smile: there");
        assert_eq!(parse_to_aliases("\u{1F44D}"), ":+1:");
    }

    #[test]
    fn skin_tones_come_off() {
        assert_eq!(parse_to_aliases("\u{1F44D}\u{1F3FD}"), ":+1:");
    }

    #[test]
    fn the_variation_selector_goes_with_it() {
        assert_eq!(parse_to_aliases("\u{2764}\u{FE0F}!"), ":heart:!");
    }

    #[test]
    fn flags_and_joined_emoji_take_their_full_length() {
        assert_eq!(parse_to_aliases("\u{1F1EC}\u{1F1E7}"), ":gb:");
        assert_eq!(parse_to_aliases("\u{1F468}\u{200D}\u{1F4BB}"), ":man_technologist:");
    }

    #[test]
    fn other_text_stays() {
        assert_eq!(parse_to_aliases("plain \u{e9}t\u{e9} \u{a7}a"), "plain \u{e9}t\u{e9} \u{a7}a");
    }
}
