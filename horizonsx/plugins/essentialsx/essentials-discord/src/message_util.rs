//! `MessageUtil`.

/// `sanitizeDiscordMarkdown`.
pub fn sanitize_discord_markdown(message: &str) -> String {
    message.replace('*', "\\*").replace('~', "\\~").replace('_', "\\_").replace('`', "\\`").replace('>', "\\>").replace('|', "\\|")
}

/// `sanitizeDiscordMarkdown`, of something that may be `null`.
pub fn sanitize_option(message: Option<&str>) -> Option<String> {
    message.map(sanitize_discord_markdown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_is_escaped() {
        assert_eq!(sanitize_discord_markdown("*a* ~b~ _c_ `d` >e |f|"), "\\*a\\* \\~b\\~ \\_c\\_ \\`d\\` \\>e \\|f\\|");
    }

    #[test]
    fn null_stays_null() {
        assert_eq!(sanitize_option(None), None);
        assert_eq!(sanitize_option(Some("_")), Some("\\_".to_owned()));
    }
}
