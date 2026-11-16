# HorizonX

A desktop app for hosting a Minecraft server on your own PC in a few clicks and letting friends join
over the internet with no port forwarding. A native Rust rewrite of what SquidServers 0.10.3 does,
running servers on the HorizonsX engine (high player counts), in the official Horizon launcher's
look.

## Intent

The owner sets up servers on his own machine to try things with friends or playtest, without
touching the router. The whole point: create, start, share one address, friends join.

## Decisions

Owner's:
- Rust, on the HorizonsX fork.
- Keep SquidServers' useful features.
- UI and background from the official Horizon launcher; nothing from SquidServers' UI.
- The playit tunnel (the playit_rs plugin) built in: no port forwarding.

Builder's calls (changeable):
- The app is a fork of the official launcher (github horizon, launcher/ at 30a7b86bc8, 2026-11-12):
  same GPUI code, Glass look, pixel-sunset sky. It keeps the launcher's version install + "prepare
  your own Minecraft files" flow, which a server needs anyway, and shares its data folder (servers,
  prepared versions and server runtime are the same as the launcher's Servers page).
- Navigation: Servers (start page), Plugins, Versions, Settings. Play, Mods, Wardrobe, Shop are gone.
- Engine: new servers run HorizonsX (`horizon-server config set engine.path=...`), bundled next to
  the app in `engines/horizonsx/`. Official Horizon stays selectable.
- Public address: playit_rs is put in every server's plugins/ and writes `plugins/playit-gg/status.json`
  (state, claim link, address); the server page shows the claim link, then the address with Copy.
  Real IPs reach the server (engine 045c5703a reads the PLAYIT header).
- Plugins: a catalog of Horizon's Rust plugins (DLLs shipped in `plugins/` beside the app), install
  and remove per server.

## Features (SquidServers -> HorizonX)

| SquidServers | HorizonX |
|---|---|
| Create server (type, version, mode, difficulty, seed, players) | Create dialog (launcher's), HorizonsX engine |
| Start / stop / restart, crash restart | launcher's (horizon-server supervisor) |
| Console + history | launcher's console + command line |
| Players: online, ops, whitelist, bans | launcher's Players tab |
| server.properties editor | launcher's Settings tab |
| Backups + restore | launcher's Backups tab |
| Performance (CPU/RAM/TPS live) | numbers (launcher's) + history graphs (new) |
| Files browser + JSON edit | Files tab (new) |
| Plugins install | Plugins page + per-server tab (new) |
| playit tunnel, claim, address | built-in playit_rs + Network card (new) |
| UPnP port mapping | later |
| Scheduled tasks | auto-backup interval + daily restart (new) |
| Server icon | Icon picker (new) |
| Accounts, friends, cloud "multihost", PostHog, Discord RPC | not taken (needs their cloud) |
| Java mods, modpacks, Geyser, Java installs | not applicable (HorizonsX runs Rust plugins) |
