# Run HorizonX with an AI assistant

HorizonX has an MCP server built in. MCP (the Model Context Protocol) is how AI assistants such as Claude Code,
Claude Desktop and Cursor use tools. Once it is connected, you can ask the assistant in plain words, for example:

- "Make a creative server called Build for my friends Alex and Sam, and start it."
- "Add EssentialsX and Chunky to every server."
- "Why is Survival lagging?"
- "Set the difficulty on Lobby to hard and the view distance to 12."
- "Put Lobby, Survival and Creative in one network with Lobby as the hub."
- "Back up Survival, then restart it."

The assistant calls the same functions that the app's buttons call, so whatever it does shows up in the open app,
and anything you do in the app is visible to it.

## 1. Connect an assistant

Open **Settings > AI assistants** in HorizonX:

| Assistant | What **Connect** does |
|---|---|
| Claude Code | Runs `claude mcp add --scope user horizonx -- <HorizonX> mcp`, so every project can use it |
| Claude Desktop | Adds `horizonx` to `claude_desktop_config.json` (restart Claude Desktop afterwards) |
| Cursor | Adds `horizonx` to `~/.cursor/mcp.json` |
| Any other MCP client | **Copy setup** copies the JSON block to paste into its MCP settings |

Other entries already in those files are kept, and the old file is saved next to it as `.json.bak`.

To set it up by hand, the server is HorizonX's own program with the argument `mcp`. It talks over stdin and stdout and
does not open a window:

```json
{
  "mcpServers": {
    "horizonx": { "command": "C:\\Program Files\\HorizonX\\HorizonX.exe", "args": ["mcp"] }
  }
}
```

## 2. What it can do (43 tools)

| Area | Tools |
|---|---|
| Overview and problems | `horizonx_overview` (every server and the network at a glance), `server_status`, `diagnose` (findings in plain words with what to do about each), `server_logs` (with a text filter), `server_report` (a performance capture) |
| Servers | `server_create` (works like the app's wizard), `server_start`, `server_stop`, `server_restart`, `server_delete`, `server_command` (any console command) |
| Settings and files | `server_settings`, `server_settings_set` (server.toml keys, checked before anything is written), `server_files`, `server_file_read`, `server_file_write` (plugin configs and similar files; the old contents are kept in `.horizon/edits/`) |
| Schedules and backups | `server_schedule`, `server_backup`, `server_backups`, `server_restore`, `server_update` |
| Players | `players`, `player_action` (op, deop, kick, ban, pardon, ban_ip, pardon_ip, whitelist add/remove/on/off) |
| Plugins | `plugin_catalog` (plugins HorizonX ships), `plugins_list`, `plugin_add` (one, some or all servers), `plugin_remove`, `plugin_set` (on/off), `plugin_add_jar`, `modrinth_search`, `modrinth_install` (any Paper/Spigot/Bukkit plugin for 26.3, with its required dependencies, checked against Modrinth's hashes) |
| Network | `network_status`, `network_create`, `network_settings`, `network_add_server`, `network_remove_server`, `network_start`, `network_stop`, `network_restart`, `network_log`, `network_delete` |
| Setup | `horizonx_setup` (installs Horizon and prepares Minecraft 26.3's files from your own copy, once) |

Every `server` argument takes the server's name (in any letter case) or its folder.

## 3. Safety

- **It asks before anything that can't be undone.** `server_delete`, `server_restore`, `plugin_remove` and `network_delete` refuse
  unless they are called with `confirm: true`. The assistant is told to ask you first.
- **It won't agree to the EULA for you.** Creating a server needs `accept_eula: true`, which the assistant is told to
  pass only after you agree to the [Minecraft EULA](https://aka.ms/MinecraftEULA).
- **Files stay inside the server.** File tools can't leave the server's folder, can only write text settings files, and
  check JSON before writing it.
- **Deleted servers go to the Recycle Bin**, not straight to deletion.
- **Servers it starts keep running** after the assistant's session ends, the same as servers started from the app.
- **Servers on another computer (over SSH)** can be started, stopped, checked and sent commands. Their files and
  plugins are only reachable on that computer.

## 4. Two other MCP servers

- `horizon-server mcp`, inside each server's runtime, moderates that one running server.
- [`sdk/mcp`](../sdk/mcp) helps an assistant write plugins (searching the SDK, examples, events): see
  [MCP-SETUP.md](MCP-SETUP.md).

HorizonX's own MCP server is the one for running servers.
