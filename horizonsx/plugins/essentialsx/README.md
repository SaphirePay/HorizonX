# EssentialsX for HorizonsX

A port of [EssentialsX](https://github.com/EssentialsX/Essentials) 2.22.0 to Rust, licensed GPL-3.0 like the original
(see `essentials/LICENSE` and `essentials/NOTICE`).

| Crate | What |
|---|---|
| `essentials` | The core: homes, warps, kits, teleports, economy, messaging, moderation and the rest of Essentials' commands ([COMMANDS.md](essentials/COMMANDS.md); what still differs: [GAPS.md](essentials/GAPS.md)) |
| `essentials-chat` | EssentialsX Chat: chat format and channels |
| `essentials-spawn` | EssentialsX Spawn: spawn point and first-join spawn |
| `essentials-protect` | EssentialsX Protect |
| `essentials-antibuild` | EssentialsX AntiBuild |
| `essentials-geoip` | EssentialsX GeoIP |
| `essentials-discord`, `essentials-discordlink` | EssentialsX Discord and DiscordLink |
| `essentials-xmpp` | EssentialsX XMPP |
| `essentials-bundle` | All of the above as the one library a server loads |

Build the bundle and drop it into a server's `plugins/` folder:

```bash
cd essentials-bundle
cargo build --release   # essentials_bundle.dll on Windows, libessentials_bundle.so on Linux
```

Leave modules out with Cargo features, e.g. `cargo build --release --no-default-features --features chat,spawn`.
