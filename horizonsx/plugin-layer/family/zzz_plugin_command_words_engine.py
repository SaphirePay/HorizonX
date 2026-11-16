# A plugin's command takes its word from the server's own extras, as a Bukkit plugin's takes a word on Paper: the
# chat filter's `/unmute` answered before the plugins' commands were asked, so FamilyFriendly's `/unmute` never ran
# (cmdparity: "help isn't muted" where Paper's plugin said "Usage: /unmute <player>").
GAME = 'horizon-client/src/game/'

EDITS = [
    (GAME + 'lan_actions.rs', '''        // The chat filter's `/unmute` (game/chat_filter.rs), on servers that filter.
        if words[0] == "unmute" && host.hub.chat_filter_on() {''', '''        // The chat filter's `/unmute` (game/chat_filter.rs), on servers that filter, unless a plugin took the word.
        if words[0] == "unmute" && host.hub.chat_filter_on() && Self::native_command_level(host, "unmute").is_none() {'''),
]
