# AxDarkAuctions 1.12.1: rewrite brief

Main class `com.artillexstudios.axdarkauctions.AxDarkAuctions`. 74 own classes, 228 KB of Java (java/).
From: D:\Horizon\plugin-sweep\user817\unzip\AxDarkAuctions-1.12.1.jar. Licence: Apache-2.0. Crate version 1.12.1.

## Commands


## Sub-commands found in the source

- none found: read the command classes

## Permissions

- `axdarkauctions.admin`
- `axdarkauctions.create`
- `axdarkauctions.delete`
- `axdarkauctions.editor`
- `axdarkauctions.forcejoin`
- `axdarkauctions.info`
- `axdarkauctions.join`
- `axdarkauctions.leave`
- `axdarkauctions.reload`
- `axdarkauctions.schedulers`
- `axdarkauctions.start`
- `axdarkauctions.stop`
- `axdarkauctions.stopall`
- `axdarkauctions.use`
- `axdarkauctions.view`

## Events

- PlayerCommandPreprocessEvent -> onCommand (com/artillexstudios/axdarkauctions/listeners/AuctionListeners.java)
- PlayerInteractEvent -> onInteract (com/artillexstudios/axdarkauctions/listeners/AuctionListeners.java)
- PlayerQuitEvent -> onQuit (com/artillexstudios/axdarkauctions/listeners/PlayerListeners.java)
- PlayerTeleportEvent -> onTeleport (com/artillexstudios/axdarkauctions/listeners/AuctionListeners.java)

## Timers


## Files

- data/auctions/example.yml
- data/config.yml
- data/currencies.yml
- data/guis/main.yml
- data/guis/preview.yml
- data/lamp-bukkit_en.properties
- data/lamp-bukkit_fr.properties
- data/lamp-bukkit_it.properties
- data/lamp-bukkit_pt.properties
- data/lamp_en.properties
- data/lamp_fr.properties
- data/lamp_it.properties
- data/lamp_pt.properties
- data/lang.yml
- data/schedulers.yml

## Config defaults found in the code

- `display.enabled`: true

## SDK map (98 calls the plugin makes, most used first: Java -> Rust)

- **kit first:** `Economy` -> hooks::economy; `InventoryClickEvent` -> gui::install routes menu clicks; `ItemStack` -> item::Spec (from config), item::to_base64; `PlaceholderAPI` -> hooks::placeholders
- `Bukkit.getConsoleSender` -> `horizon_bukkit::get_console_sender` `fn get_console_sender() -> CommandSender`
- `Player.sendMessage` -> `horizon_bukkit::Player::send_component` `fn send_component(&self, text: &Component)`
- `Bukkit.getPlayer` -> `horizon_bukkit::fam_ess::get_player_by_id` `fn get_player_by_id(uuid: &str) -> Option<Player>`
- `ItemStack.add` -> `horizon_bukkit::ItemStack::add` `fn add(&mut self, amount: i32) -> &mut Self`  (kit: item::Spec (from config), item::to_base64)
- `ItemStack.of` -> `horizon_bukkit::ItemStack::new` `fn new(material: Material, amount: i32) -> Self`  (kit: item::Spec (from config), item::to_base64)
- `Bukkit.reload` -> `horizon_bukkit::fam_core::reload` `fn reload()`
- `Player.getLocation` -> `horizon_bukkit::Player::get_location` `fn get_location(&self) -> Location`
- `Bukkit.getOfflinePlayer` -> `horizon_bukkit::get_offline_player` `fn get_offline_player(name: &str) -> OfflinePlayer`
- `ItemStack.clone` -> `horizon_bukkit::ItemStack::copy` `fn copy(&self) -> Self`  (kit: item::Spec (from config), item::to_base64)
- `Player.getInventory` -> `horizon_bukkit::Player::get_inventory` `fn get_inventory(&self) -> PlayerInventory`
- `Player.getName` -> `horizon_bukkit::Player::get_name` `fn get_name(&self) -> &str`
- `Bukkit.getName` -> `horizon_bukkit::get_name` `fn get_name() -> &'static str`
- `ItemStack.getItemMeta` -> `horizon_bukkit::ItemStack::meta` `fn meta<M: MetaClass>(&self) -> M`  (kit: item::Spec (from config), item::to_base64)
- `ItemStack.getType` -> `horizon_bukkit::ItemStack::get_type` `fn get_type(&self) -> &Material`  (kit: item::Spec (from config), item::to_base64)
- `ItemStack.isEmpty` -> `horizon_bukkit::ItemStack::is_empty` `fn is_empty(&self) -> bool`  (kit: item::Spec (from config), item::to_base64)
- `ItemStack.setAmount` -> `horizon_bukkit::ItemStack::set_amount` `fn set_amount(&mut self, amount: i32)`  (kit: item::Spec (from config), item::to_base64)
- `ItemStack.setLore` -> `horizon_bukkit::ItemStack::set_lore` `fn set_lore(&mut self, lore: Vec<String>)`  (kit: item::Spec (from config), item::to_base64)
- `Player.closeInventory` -> `horizon_bukkit::Player::close_inventory` `fn close_inventory(&self)`
- `Player.getUniqueId` -> `horizon_bukkit::Player::get_unique_id` `fn get_unique_id(&self) -> String`
- `Bukkit.getPluginManager` -> `horizon_bukkit::fam_core::get_plugin_manager` `fn get_plugin_manager() -> PluginManager`
- `ItemStack.serialize` -> `horizon_bukkit::ItemStack::serialize` `fn serialize(&self) -> BTreeMap<String, Value>`  (kit: item::Spec (from config), item::to_base64)
- `Material.isEmpty` -> `horizon_bukkit::Material::is_empty` `fn is_empty(&self) -> bool`
- `Bukkit.dispatchCommand` -> `horizon_bukkit::fam_core::dispatch_command_as` `fn dispatch_command_as(sender: &CommandSender, line: &str) -> bool`
- `Bukkit.getServicesManager` -> `horizon_bukkit::persistence::get_services_manager` `fn get_services_manager() -> ServicesManager`
- `Bukkit.isPrimaryThread` -> `horizon_bukkit::is_primary_thread` `fn is_primary_thread() -> bool`
- `CommandSender.sendMessage` -> `horizon_bukkit::CommandSender::send_component` `fn send_component(&self, text: &Component)`
- `HumanEntity.closeInventory` -> `horizon_bukkit::Player::close_inventory` `fn close_inventory(&self)`
- `InventoryClickEvent.isLeftClick` -> `horizon_bukkit::fam_ev_player::InventoryClickEvent::is_left_click` `fn is_left_click(&self) -> bool`  (kit: gui::install routes menu clicks)
- `InventoryClickEvent.isRightClick` -> `horizon_bukkit::fam_ev_player::InventoryClickEvent::is_right_click` `fn is_right_click(&self) -> bool`  (kit: gui::install routes menu clicks)
- `InventoryClickEvent.isShiftClick` -> `horizon_bukkit::fam_ev_player::InventoryClickEvent::is_shift_click` `fn is_shift_click(&self) -> bool`  (kit: gui::install routes menu clicks)
- `ItemMeta.getLore` -> `horizon_bukkit::ItemStack::get_lore` `fn get_lore(&self) -> Vec<String>`
- `ItemMeta.setLore` -> `horizon_bukkit::ItemStack::set_lore` `fn set_lore(&mut self, lore: Vec<String>)`
- `ItemStack.deserialize` -> `horizon_bukkit::ItemStack::deserialize` `fn deserialize(saved: &BTreeMap<String, Value>) -> Option<Self>`  (kit: item::Spec (from config), item::to_base64)
- `ItemStack.getAmount` -> `horizon_bukkit::ItemStack::get_amount` `fn get_amount(&self) -> i32`  (kit: item::Spec (from config), item::to_base64)
- `ItemStack.getLore` -> `horizon_bukkit::ItemStack::get_lore` `fn get_lore(&self) -> Vec<String>`  (kit: item::Spec (from config), item::to_base64)
- `ItemStack.setItemMeta` -> `horizon_bukkit::ItemStack::set_display_name` `fn set_display_name(&mut self, name: &str)`  (kit: item::Spec (from config), item::to_base64)
- `Location.add` -> `horizon_bukkit::Location::add` `fn add(&mut self, x: f64, y: f64, z: f64) -> &mut Self`
- `Location.clone` -> `horizon_bukkit::Location::copy` `fn copy(&self) -> Self`
- `Location.deserialize` -> `horizon_bukkit::Location::deserialize` `fn deserialize(map: &Value) -> Option<Self>`
- `Location.getYaw` -> `horizon_bukkit::Location::get_yaw` `fn get_yaw(&self) -> f32`
- `Material.getKey` -> `horizon_bukkit::Material::get_key` `fn get_key(&self) -> &str`
- `Material.name` -> `horizon_bukkit::Material::name` `fn name(&self) -> String`
- `Player.hidePlayer` -> `horizon_bukkit::Player::hide_player` `fn hide_player(&self, other: &Player)`
- `Player.showPlayer` -> `horizon_bukkit::Player::show_player` `fn show_player(&self, other: &Player)`
- `Player.teleport` -> `horizon_bukkit::Player::teleport` `fn teleport(&self, to: &Location) -> bool` ~
- `PlayerInteractEvent.setCancelled` -> `horizon_bukkit::fam_ev_player::PlayerInteractEvent::set_cancelled` `fn set_cancelled(&mut self, cancelled: bool)`
- `AsyncPlayerChatEvent.getMessage` -> `horizon_bukkit::fam_ev_player::AsyncPlayerChatEvent::get_message` `fn get_message(&self) -> &str`
- `AsyncPlayerChatEvent.setCancelled` -> `horizon_bukkit::fam_ev_player::AsyncPlayerChatEvent::set_cancelled` `fn set_cancelled(&mut self, cancelled: bool)`
- `Bukkit.broadcast` -> `horizon_bukkit::server::broadcast` `fn broadcast(text: &Component)`
- `Bukkit.getBannedPlayers` -> `horizon_bukkit::server::get_banned_players` `fn get_banned_players() -> Vec<OfflinePlayer>`
- `Bukkit.getOnlinePlayers` -> `horizon_bukkit::get_online_players` `fn get_online_players() -> Vec<Player>`
- `Bukkit.getVersion` -> `horizon_bukkit::get_version` `fn get_version() -> String`
- `ClickType.isRightClick` -> `horizon_bukkit::fam_ev_player::is_right_click` `fn is_right_click(click: &str) -> bool`
- `CommandSender.hasPermission` -> `horizon_bukkit::CommandSender::has_permission` `fn has_permission(&self, node: &str) -> bool`
- `Conversation.begin` -> `horizon_bukkit::fam_core::FullConversation::begin` `fn begin(mut self, plugin: &Plugin)`
- `Conversation.getForWhom` -> `horizon_bukkit::fam_core::FullConversation::get_for_whom` `fn get_for_whom(&self) -> &Player`
- `ConversationContext.getForWhom` -> `horizon_bukkit::fam_core::FullConversation::get_for_whom` `fn get_for_whom(&self) -> &Player`
- `ConversationFactory.buildConversation` -> `horizon_bukkit::fam_core::ConversationFactory::build_conversation` `fn build_conversation(mut self, whom: &CommandSender) -> Option<FullConversation>`
- `ConversationFactory.withFirstPrompt` -> `horizon_bukkit::fam_core::ConversationFactory::with_first_prompt` `fn with_first_prompt(mut self, first: impl Fn() -> Box<dyn crate::pure::Prompt> + Send + Sync + 'static) -> Self`
- `Entity.getLocation` -> `horizon_bukkit::Player::get_location` `fn get_location(&self) -> Location`
- `Entity.getName` -> `horizon_bukkit::entity::Entity::get_name` `fn get_name(&self) -> String`
- `Entity.getUniqueId` -> `horizon_bukkit::Player::get_unique_id` `fn get_unique_id(&self) -> String`
- `Entity.remove` -> `horizon_bukkit::entity::Entity::remove` `fn remove(&self) -> bool`
- `Entity.teleport` -> `horizon_bukkit::entity::Entity::teleport` `fn teleport(&self, to: &Location) -> bool`
- `Entity.teleportAsync` -> `horizon_bukkit::entity::Entity::teleport` `fn teleport(&self, to: &Location) -> bool`
- `EntityType.valueOf` -> `horizon_bukkit::enums::entity_type::value_of` `fn value_of(name: &str) -> Option<&'static str>`
- `EntityType.values` -> `horizon_bukkit::enums::entity_type::values` `fn values() -> &'static [&'static str]`
- `EventPriority.values` -> `horizon_bukkit::enums::event_priority::values` `fn values() -> &'static [&'static str]`
- `HumanEntity.getInventory` -> `horizon_bukkit::Player::get_inventory` `fn get_inventory(&self) -> PlayerInventory`
- `HumanEntity.getName` -> `horizon_bukkit::Player::get_name` `fn get_name(&self) -> &str`
- `ItemStack.empty` -> `horizon_bukkit::ItemStack::empty` `fn empty() -> Self`  (kit: item::Spec (from config), item::to_base64)
- `Location.getPitch` -> `horizon_bukkit::Location::get_pitch` `fn get_pitch(&self) -> f32`
- `Location.getX` -> `horizon_bukkit::Location::get_x` `fn get_x(&self) -> f64`
- `Location.getY` -> `horizon_bukkit::Location::get_y` `fn get_y(&self) -> f64`
- `Location.getZ` -> `horizon_bukkit::Location::get_z` `fn get_z(&self) -> f64`
- `Location.setPitch` -> `horizon_bukkit::Location::set_pitch` `fn set_pitch(&mut self, pitch: f32)`
- `Location.setX` -> `horizon_bukkit::Location::set_x` `fn set_x(&mut self, x: f64)`
- `Location.setY` -> `horizon_bukkit::Location::set_y` `fn set_y(&mut self, y: f64)`
- `Location.setYaw` -> `horizon_bukkit::Location::set_yaw` `fn set_yaw(&mut self, yaw: f32)`
- `Location.setZ` -> `horizon_bukkit::Location::set_z` `fn set_z(&mut self, z: f64)`
- `Material.isAir` -> `horizon_bukkit::Material::is_air` `fn is_air(&self) -> bool`
- `Material.values` -> `horizon_bukkit::enums::material::values` `fn values() -> &'static [&'static str]`
- `OfflinePlayer.getName` -> `horizon_bukkit::OfflinePlayer::get_name` `fn get_name(&self) -> &str`
- `OfflinePlayer.getPlayer` -> `horizon_bukkit::OfflinePlayer::get_player` `fn get_player(&self) -> Option<Player>`
- `Player.getDisplayName` -> `horizon_bukkit::Player::get_display_name` `fn get_display_name(&self) -> &str`
- `Player.getExp` -> `horizon_bukkit::Player::get_exp` `fn get_exp(&self) -> f32`
- `Player.getLevel` -> `horizon_bukkit::Player::get_level` `fn get_level(&self) -> i32`
- `Player.isOnline` -> `horizon_bukkit::Player::is_online` `fn is_online(&self) -> bool`
- `Player.sendRawMessage` -> `horizon_bukkit::Player::send_raw_message` `fn send_raw_message(&self, text: &str)`
- `Player.setExp` -> `horizon_bukkit::Player::set_exp` `fn set_exp(&self, progress: f32)`
- `Player.setLevel` -> `horizon_bukkit::Player::set_level` `fn set_level(&self, level: i32)`
- `PlayerCommandPreprocessEvent.getMessage` -> `horizon_bukkit::event::PlayerCommandPreprocessEvent::get_message` `fn get_message(&self) -> &str`
- `PlayerCommandPreprocessEvent.setCancelled` -> `horizon_bukkit::event::PlayerCommandPreprocessEvent::set_cancelled` `fn set_cancelled(&mut self, cancelled: bool)`
- `PlayerInteractEvent.getClickedBlock` -> `horizon_bukkit::fam_ev_player::PlayerInteractEvent::get_clicked_block` `fn get_clicked_block(&self) -> Option<&Block>`
- `Plugin.getName` -> `horizon_bukkit::Plugin::get_name` `fn get_name(&self) -> &str`
- `RegisteredServiceProvider.getProvider` -> `horizon_bukkit::persistence::ServiceProvider::call` `fn call(&self, method: &str, args: &horizon_mod::Value) -> horizon_mod::Value`
- `SkullMeta.setOwningPlayer` -> `horizon_bukkit::fam_items::set_owning_player` `fn set_owning_player(&mut self, player: &crate::OfflinePlayer) -> bool`

## Bukkit classes it imports that the SDK has no rows for (look these up, or work around)

- Prompt, StringPrompt

## Keys read from config / messages

- `bid-slots`
- `broadcasts.`
- `commands.console-only`
- `commands.invalid-command`
- `commands.invalid-player`
- `commands.invalid-selector`
- `commands.invalid-value`
- `commands.missing-argument`
- `commands.no-permission`
- `commands.out-of-range`
- `commands.player-only`
- `lang.yml`
- `language`
- `location`
- `locations.display`
- `locations.exit`
- `locations.spawn`
- `material`
- `material.`
- `name`
- `not-found`
- `npcs.`
- `prefix`
- `sell-item-amount`
- `settings.give-command`
- `settings.raw-placeholder`
- `settings.take-command`
- `texture`
- `timezone`
- `title`
- `type`

## Biggest classes (write these first, they are most of the work)

- 15 KB com/artillexstudios/axdarkauctions/auctions/Auction.java
- 14 KB com/artillexstudios/axdarkauctions/hooks/HookManager.java
- 13 KB com/artillexstudios/axdarkauctions/auctions/AuctionEntities.java
- 12 KB com/artillexstudios/axdarkauctions/gui/impl/AuctionGui.java
- 12 KB com/artillexstudios/axdarkauctions/editor/EditorBase2.java
- 11 KB com/artillexstudios/axdarkauctions/editor/EditorBase.java
- 7 KB com/artillexstudios/axdarkauctions/AxDarkAuctions.java
- 7 KB com/artillexstudios/axdarkauctions/editor/impl/RewardEditor.java
- 5 KB com/artillexstudios/axdarkauctions/schedulers/AuctionScheduler.java
- 5 KB com/artillexstudios/axdarkauctions/editor/impl/Scheduler2Editor.java
- 5 KB com/artillexstudios/axdarkauctions/editor/impl/MainEditor.java
- 5 KB com/artillexstudios/axdarkauctions/gui/GuiFrame.java
- 5 KB com/artillexstudios/axdarkauctions/editor/impl/RewardSettingEditor.java
- 5 KB com/artillexstudios/axdarkauctions/auctions/AuctionSettings.java
- 5 KB com/artillexstudios/axdarkauctions/commands/MainCommand.java
- 4 KB com/artillexstudios/axdarkauctions/editor/impl/SchedulerEditor.java
- 4 KB com/artillexstudios/axdarkauctions/libraries/Libraries.java
- 3 KB com/artillexstudios/axdarkauctions/commands/subcommands/Reload.java
- 3 KB com/artillexstudios/axdarkauctions/auctions/AuctionPlayers.java
- 3 KB com/artillexstudios/axdarkauctions/lang/LanguageManager.java
- 3 KB com/artillexstudios/axdarkauctions/gui/impl/PreviewGui.java
- 3 KB com/artillexstudios/axdarkauctions/editor/impl/NPCEditor.java
- 2 KB com/artillexstudios/axdarkauctions/hooks/currency/UltraEconomyHook.java
- 2 KB com/artillexstudios/axdarkauctions/hooks/other/PlaceholderAPIHook.java
- 2 KB com/artillexstudios/axdarkauctions/hooks/currency/ExperienceHook.java

## Kit map (`use horizon_kit::prelude::*;` brings all of it; recipes in KIT.md)

- messages: `msg("key", &[args])` (Lang: {prefix}, %s, {0}); settings: `config(|c| c.int("path", default))`; any YAML node: `yaml::{string, list_or_one, int, is_section, keys}`
- commands: the Router above; a sub = `fn(&Ctx) -> Res` (`c.player()?`, `c.int(0, "amount")?`, `c.online(0)?`, `c.tell(..)`)
- menus: `gui::install(plugin)` once, then `gui::Menu::new(rows, title).button(slot, item, |c| ..).open(&p)`; lists: `gui::Pages`; `gui::slots("9-17")`
- items from config: `item::Spec::from_yaml(&node).build(&fill)`; saved items: `item::to_base64` / `from_base64`; heads: `item::head`
- player data: `store::Docs<T>` (one JSON per uuid, load at pre-login, batched off-thread saves); one file: `store::File<T>`
- money / ranks / placeholders: `hooks::economy`, `hooks::chat`, `hooks::placeholders(text, Some(&p))`, offer yours: `hooks::provide_placeholders`
- text: `mini::parse_mixed` (MiniMessage + & codes) -> component; `text::colorize` -> legacy; HOCON: `hocon::read`
- durations `time::parse("10m")`, `sync::{on_main, drain, hold, Cooldowns}`, sounds `sound::play(&p, "ENTITY_X:1:1")`
- client-only entities, scores, boss bars: `packets`, `wire` (`wire::set_score`, `wire::boss`); per-tick work: `worker`
- a `/x profiler`: `stats::{set_running, report, timed}`; anything else: the SDK map above, then `api_lookup <Class.method>`
