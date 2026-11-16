# LPC 4.4.0: rewrite brief

Main class `de.ayont.lpc.LPC`. 23 own classes, 69 KB of Java (java/).

## Commands

- `/lpc`: LPC chat formatter commands.

## Sub-commands found in the source

- `mute` (de/ayont/lpc/commands/LPCCommand.java:40)
- `reload` (de/ayont/lpc/commands/LPCCommand.java:34)
- `unmute` (de/ayont/lpc/commands/LPCCommand.java:43)
- `version` (de/ayont/lpc/commands/LPCCommand.java:37)

## Permissions

- `lpc.bypass.advert`
- `lpc.bypass.caps`
- `lpc.bypass.profanity`
- `lpc.bypass.repeat`
- `lpc.bypass.spam`
- `lpc.chatcolor`
- `lpc.chatlinks`
- `lpc.emoji`
- `lpc.itemplaceholder`
- `lpc.mention.exempt`
- `lpc.mute`
- `lpc.muted`
- `lpc.reload`
- `lpc.update`

## Events

- AsyncChatEvent -> onChat (de/ayont/lpc/listener/AsyncChatListener.java)
- PlayerDeathEvent -> onDeath (de/ayont/lpc/listener/ConnectionListener.java)
- PlayerJoinEvent -> onJoin (de/ayont/lpc/listener/ConnectionListener.java)
- PlayerJoinEvent -> onJoin (de/ayont/lpc/update/UpdateChecker.java)
- PlayerQuitEvent -> onQuit (de/ayont/lpc/listener/ConnectionListener.java)

## Timers


## Files

- data/lpc-build.properties
- data/config.yml

## Keys read from config / messages

- `anti-advert.action`
- `anti-advert.mask`
- `anti-advert.message`
- `anti-spam.message`
- `caps-filter.action`
- `caps-filter.message`
- `chat-format`
- `death-messages.format`
- `default-message-style`
- `gradient-names.default-meta-key`
- `join-messages.first-join.format`
- `join-messages.format`
- `link-urls.hover-text`
- `link-urls.style`
- `mentions.actionbar-text`
- `mentions.highlight-format`
- `mentions.ping-sound`
- `mute.message`
- `mute.permission-node`
- `profanity-filter.action`
- `profanity-filter.mask-char`
- `profanity-filter.message`
- `quit-messages.format`
- `reload-message`
- `repeat-filter.message`

## Biggest classes (write these first, they are most of the work)

- 8 KB de/ayont/lpc/chat/ChatFormatService.java
- 8 KB de/ayont/lpc/commands/LPCCommand.java
- 6 KB de/ayont/lpc/moderation/ModerationService.java
- 6 KB de/ayont/lpc/LPC.java
- 5 KB de/ayont/lpc/chat/MentionService.java
- 4 KB de/ayont/lpc/moderation/TextChecks.java
- 3 KB de/ayont/lpc/update/UpdateChecker.java
- 3 KB de/ayont/lpc/listener/ConnectionListener.java
- 2 KB de/ayont/lpc/chat/UrlLinkifier.java
- 2 KB de/ayont/lpc/listener/AsyncChatListener.java
- 2 KB de/ayont/lpc/listener/SpigotChatListener.java
- 2 KB de/ayont/lpc/moderation/MuteService.java
- 1 KB de/ayont/lpc/chat/EmojiReplacer.java
- 1 KB de/ayont/lpc/chat/ItemPlaceholder.java
- 1 KB de/ayont/lpc/chat/LegacyColors.java
- 1 KB de/ayont/lpc/chat/PlayerMessages.java
- 1 KB de/ayont/lpc/scheduler/PaperSchedulerImpl.java
- 1 KB de/ayont/lpc/moderation/ModResult.java
- 1 KB de/ayont/lpc/update/Versions.java
- 0 KB de/ayont/lpc/scheduler/BukkitSchedulerImpl.java
- 0 KB de/ayont/lpc/chat/ComponentSanitizer.java
- 0 KB de/ayont/lpc/scheduler/Schedulers.java
- 0 KB de/ayont/lpc/scheduler/Scheduler.java

## Kit map

- messages: `msg("key", &[args])` (Lang: {prefix}, %s, {0}); settings: `config(|c| c.int("path", default))`
- commands: the Router above; a sub = `fn(&Ctx) -> Res` (`c.player()?`, `c.int(0, "amount")?`, `c.online(0)?`, `c.tell(..)`)
- client-only entities (holograms, markers, NPCs): `horizon_kit::packets` (+ `wire` for raw bytes)
- per-tick work: `horizon_kit::worker` (server-thread cost ~0, as Java async)
- a `/x profiler`: `horizon_kit::stats::{set_running, report, timed}`
- any Bukkit call: `mcp plugin-port api_lookup <Class.method>` / `sdk_search`
