# DeluxeMenus 1.14.1-Release: rewrite brief

Main class `com.extendedclip.deluxemenus.DeluxeMenus`. 83 own classes, 436 KB of Java (java/).

## Commands

- `/deluxemenus` aliases ['dm', 'deluxemenu', 'dmenu']: DeluxeMenus main commands

## Sub-commands found in the source

- none found: read the command classes

## Permissions

- `deluxemenus.admin`
- `deluxemenus.list`
- `deluxemenus.menu.*`
- `deluxemenus.open`
- `deluxemenus.open.bypass`
- `deluxemenus.open.others`
- `deluxemenus.openrequirement.bypass.*`
- `deluxemenus.placeholdersfor`
- `deluxemenus.placeholdersfor.exempt`
- `deluxemenus.reload`

## Events

- InventoryCloseEvent -> onClose (com/extendedclip/deluxemenus/listener/PlayerListener.java)
- InventoryOpenEvent -> onOpen (com/extendedclip/deluxemenus/listener/PlayerListener.java)
- PlayerQuitEvent -> onLeave (com/extendedclip/deluxemenus/listener/PlayerListener.java)

## Timers


## Files

- data/items_menu.yml
- data/basics_menu.yml
- data/requirements_menu.yml
- data/advanced_menu.yml
- data/actions_menu.yml
- data/default_menu.yml

## Keys read from config / messages

- `debug`
- `gui_menus.`

## Biggest classes (write these first, they are most of the work)

- 69 KB com/extendedclip/deluxemenus/config/DeluxeMenusConfig.java
- 27 KB com/extendedclip/deluxemenus/command/subcommand/MetaCommand.java
- 25 KB com/extendedclip/deluxemenus/menu/options/MenuItemOptions.java
- 23 KB com/extendedclip/deluxemenus/menu/MenuItem.java
- 19 KB com/extendedclip/deluxemenus/utils/Messages.java
- 16 KB com/extendedclip/deluxemenus/action/ClickActionTask.java
- 15 KB com/extendedclip/deluxemenus/command/subcommand/ListCommand.java
- 14 KB com/extendedclip/deluxemenus/DeluxeMenus.java
- 13 KB com/extendedclip/deluxemenus/menu/Menu.java
- 12 KB com/extendedclip/deluxemenus/nbt/NbtProvider.java
- 11 KB com/extendedclip/deluxemenus/persistentmeta/PersistentMetaHandler.java
- 11 KB com/extendedclip/deluxemenus/requirement/HasItemRequirement.java
- 9 KB com/extendedclip/deluxemenus/menu/MenuHolder.java
- 7 KB com/extendedclip/deluxemenus/command/subcommand/OpenCommand.java
- 7 KB com/extendedclip/deluxemenus/menu/options/MenuOptions.java
- 7 KB com/extendedclip/deluxemenus/utils/DumpUtils.java
- 6 KB com/extendedclip/deluxemenus/listener/PlayerListener.java
- 6 KB com/extendedclip/deluxemenus/requirement/RequirementType.java
- 6 KB com/extendedclip/deluxemenus/utils/SkullUtils.java
- 5 KB com/extendedclip/deluxemenus/menu/command/RegistrableMenuCommand.java
- 5 KB com/extendedclip/deluxemenus/placeholder/Expansion.java
- 5 KB com/extendedclip/deluxemenus/utils/VersionHelper.java
- 4 KB com/extendedclip/deluxemenus/action/ActionType.java
- 4 KB com/extendedclip/deluxemenus/command/subcommand/ExecuteCommand.java
- 4 KB com/extendedclip/deluxemenus/command/DeluxeMenusCommand.java

## Kit map

- messages: `msg("key", &[args])` (Lang: {prefix}, %s, {0}); settings: `config(|c| c.int("path", default))`
- commands: the Router above; a sub = `fn(&Ctx) -> Res` (`c.player()?`, `c.int(0, "amount")?`, `c.online(0)?`, `c.tell(..)`)
- client-only entities (holograms, markers, NPCs): `horizon_kit::packets` (+ `wire` for raw bytes)
- per-tick work: `horizon_kit::worker` (server-thread cost ~0, as Java async)
- a `/x profiler`: `horizon_kit::stats::{set_running, report, timed}`
- any Bukkit call: `mcp plugin-port api_lookup <Class.method>` / `sdk_search`
