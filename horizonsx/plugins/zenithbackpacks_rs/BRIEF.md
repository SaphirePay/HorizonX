# ZenithBackpacks ${version}: rewrite brief

Main class `com.zenithbackpacks.ZenithBackpacksPlugin`. 45 own classes, 135 KB of source (java/).
From: the ZenithBackpacks source. Licence: not found (check before porting). Crate version 0.1.0.

## Commands

- `/zenithbackpacks` aliases ['zbpack', 'zbackpack']: How to use the Zenith backpack item
- `/zenithbackpacksadmin`, permission `zenithbackpacks.admin`: Admin reload, give backpack, give vault chest, purge orphans

## Sub-commands found in the source

- none found: read the command classes

## Permissions

- `zenithbackpacks.admin`
- `zenithbackpacks.bypass`
- `zenithbackpacks.chest.bypass`
- `zenithbackpacks.rows.1`
- `zenithbackpacks.rows.2`
- `zenithbackpacks.rows.3`
- `zenithbackpacks.rows.4`
- `zenithbackpacks.rows.5`
- `zenithbackpacks.rows.6`
- `zenithbackpacks.upgrade`
- `zenithbackpacks.use`

## Events

- AsyncChatEvent -> onChat (com/zenithbackpacks/chest/ChatPrompt.java)
- BlockBreakEvent -> onBreak (com/zenithbackpacks/listener/ChestBlockListener.java)
- BlockExplodeEvent -> onBlockExplode (com/zenithbackpacks/listener/ChestBlockListener.java)
- BlockPlaceEvent -> onPlace (com/zenithbackpacks/listener/ChestBlockListener.java)
- EntityExplodeEvent -> onEntityExplode (com/zenithbackpacks/listener/ChestBlockListener.java)
- InventoryClickEvent -> onClick (com/zenithbackpacks/listener/BackpackGuiListener.java)
- InventoryClickEvent -> onClick (com/zenithbackpacks/listener/ChestGuiListener.java)
- InventoryCloseEvent -> onClose (com/zenithbackpacks/listener/BackpackGuiListener.java)
- InventoryCloseEvent -> onClose (com/zenithbackpacks/listener/ChestGuiListener.java)
- InventoryDragEvent -> onDrag (com/zenithbackpacks/listener/BackpackGuiListener.java)
- InventoryDragEvent -> onDrag (com/zenithbackpacks/listener/ChestGuiListener.java)
- InventoryMoveItemEvent -> onHopper (com/zenithbackpacks/listener/ChestBlockListener.java)
- PlayerInteractEvent -> onInteract (com/zenithbackpacks/listener/BackpackInteractListener.java)
- PlayerInteractEvent -> onInteract (com/zenithbackpacks/listener/ChestInteractListener.java)
- PlayerQuitEvent -> onQuit (com/zenithbackpacks/chest/ChatPrompt.java)
- PlayerQuitEvent -> onQuit (com/zenithbackpacks/listener/PlayerQuitListener.java)

## Timers


## Files

- data/config.yml
- data/gui.yml
- data/messages.yml

## SDK map (140 calls the plugin makes, most used first: Java -> Rust)

- **kit first:** `Component` -> mini::parse for MiniMessage text; `ConfigurationSection` -> yaml::{string,list_or_one,int,is_section,keys}; `Economy` -> hooks::economy; `FileConfiguration` -> Config / yaml; `Inventory` -> gui::Menu (buttons, pages, click routing); `InventoryClickEvent` -> gui::install routes menu clicks; `ItemStack` -> item::Spec (from config), item::to_base64; `MiniMessage` -> mini::parse / parse_mixed (gradients, hover, click); `YamlConfiguration` -> Config::load / yaml
- `ItemStack.add` -> `horizon_bukkit::ItemStack::add` `fn add(&mut self, amount: i32) -> &mut Self`  (kit: item::Spec (from config), item::to_base64)
- `ItemStack.getType` -> `horizon_bukkit::ItemStack::get_type` `fn get_type(&self) -> &Material`  (kit: item::Spec (from config), item::to_base64)
- `Player.getUniqueId` -> `horizon_bukkit::Player::get_unique_id` `fn get_unique_id(&self) -> String`
- `MiniMessage.deserialize` -> `horizon_bukkit::adventure::mini_message` `fn mini_message(input: &str) -> Component` ~  (kit: mini::parse / parse_mixed (gradients, hover, click))
- `Player.hasPermission` -> `horizon_bukkit::Player::has_permission` `fn has_permission(&self, node: &str) -> bool`
- `Component.empty` -> `horizon_bukkit::Component::empty` `fn empty() -> Self`  (kit: mini::parse for MiniMessage text)
- `ItemStack.deserialize` -> `horizon_bukkit::ItemStack::deserialize` `fn deserialize(saved: &BTreeMap<String, Value>) -> Option<Self>`  (kit: item::Spec (from config), item::to_base64)
- `Player.sendMessage` -> `horizon_bukkit::Player::send_component` `fn send_component(&self, text: &Component)`
- `ItemMeta.displayName` -> `horizon_bukkit::ItemStack::display_name` `fn display_name(&self) -> Option<Component>`
- `ItemMeta.lore` -> `horizon_bukkit::ItemStack::lore` `fn lore(&self) -> Vec<Component>`
- `ItemStack.clone` -> `horizon_bukkit::ItemStack::copy` `fn copy(&self) -> Self`  (kit: item::Spec (from config), item::to_base64)
- `ItemStack.displayName` -> `horizon_bukkit::ItemStack::display_name` `fn display_name(&self) -> Option<Component>`  (kit: item::Spec (from config), item::to_base64)
- `ItemStack.empty` -> `horizon_bukkit::ItemStack::empty` `fn empty() -> Self`  (kit: item::Spec (from config), item::to_base64)
- `ItemStack.getItemMeta` -> `horizon_bukkit::ItemStack::meta` `fn meta<M: MetaClass>(&self) -> M`  (kit: item::Spec (from config), item::to_base64)
- `ItemStack.lore` -> `horizon_bukkit::ItemStack::lore` `fn lore(&self) -> Vec<Component>`  (kit: item::Spec (from config), item::to_base64)
- `ItemStack.setItemMeta` -> `horizon_bukkit::ItemStack::set_display_name` `fn set_display_name(&mut self, name: &str)`  (kit: item::Spec (from config), item::to_base64)
- `Material.valueOf` -> `horizon_bukkit::Material::of` `fn of(name: &str) -> Self`
- `YamlConfiguration.loadConfiguration` -> `horizon_bukkit::config::YamlConfiguration::load_configuration` `fn load_configuration(path: &Path) -> Self`  (kit: Config::load / yaml)
- `Bukkit.createInventory` -> `horizon_bukkit::Inventory::create` `fn create(size: usize, title: &str) -> Self`
- `Inventory.getContents` -> `horizon_bukkit::Inventory::get_contents` `fn get_contents(&self) -> &[Option<ItemStack>]`  (kit: gui::Menu (buttons, pages, click routing))
- `Inventory.getType` -> `horizon_bukkit::Inventory::get_type` `fn get_type(&self) -> &'static str`  (kit: gui::Menu (buttons, pages, click routing))
- `Inventory.setItem` -> `horizon_bukkit::Inventory::set_item` `fn set_item(&mut self, slot: usize, stack: Option<ItemStack>)`  (kit: gui::Menu (buttons, pages, click routing))
- `ItemStack.isEmpty` -> `horizon_bukkit::ItemStack::is_empty` `fn is_empty(&self) -> bool`  (kit: item::Spec (from config), item::to_base64)
- `Material.isEmpty` -> `horizon_bukkit::Material::is_empty` `fn is_empty(&self) -> bool`
- `Player.getInventory` -> `horizon_bukkit::Player::get_inventory` `fn get_inventory(&self) -> PlayerInventory`
- `Player.openInventory` -> `horizon_bukkit::Player::open_inventory` `fn open_inventory(&self, inventory: &Inventory)` ~
- `ConfigurationSection.createSection` -> `horizon_bukkit::config::YamlConfiguration::create_section` `fn create_section(&mut self, path: &str, section: Value)`  (kit: yaml::{string,list_or_one,int,is_section,keys})
- `ConfigurationSection.get` -> `horizon_bukkit::config::YamlConfiguration::get` `fn get(&self, path: &str) -> Option<Value>`  (kit: yaml::{string,list_or_one,int,is_section,keys})
- `ConfigurationSection.getConfigurationSection` -> `horizon_bukkit::config::YamlConfiguration::get_configuration_section` `fn get_configuration_section(&self, path: &str) -> Option<Self>`  (kit: yaml::{string,list_or_one,int,is_section,keys})
- `ConfigurationSection.getInt` -> `horizon_bukkit::config::YamlConfiguration::get_int` `fn get_int(&self, path: &str) -> i32`  (kit: yaml::{string,list_or_one,int,is_section,keys})
- `ConfigurationSection.getKeys` -> `horizon_bukkit::config::YamlConfiguration::get_keys` `fn get_keys(&self, path: &str) -> Vec<String>`  (kit: yaml::{string,list_or_one,int,is_section,keys})
- `ConfigurationSection.set` -> `horizon_bukkit::config::YamlConfiguration::set` `fn set(&mut self, path: &str, value: Value)`  (kit: yaml::{string,list_or_one,int,is_section,keys})
- `Inventory.isEmpty` -> `horizon_bukkit::Inventory::is_empty` `fn is_empty(&self) -> bool`  (kit: gui::Menu (buttons, pages, click routing))
- `Location.getWorld` -> `horizon_bukkit::Location::get_world` `fn get_world(&self) -> &World`
- `Player.displayName` -> `horizon_bukkit::Player::display_name` `fn display_name(&self) -> Component`
- `Block.getLocation` -> `horizon_bukkit::Block::get_location` `fn get_location(&self) -> Location`
- `Block.getWorld` -> `horizon_bukkit::Block::get_world` `fn get_world(&self) -> &World`
- `Block.setType` -> `horizon_bukkit::Block::set_type` `fn set_type(&self, material: &Material) -> bool`
- `Bukkit.getName` -> `horizon_bukkit::get_name` `fn get_name() -> &'static str`
- `Bukkit.getPlayer` -> `horizon_bukkit::fam_ess::get_player_by_id` `fn get_player_by_id(uuid: &str) -> Option<Player>`
- `Bukkit.getWorld` -> `horizon_bukkit::get_world` `fn get_world(name: &str) -> Option<World>`
- `CommandSender.hasPermission` -> `horizon_bukkit::CommandSender::has_permission` `fn has_permission(&self, node: &str) -> bool`
- `CommandSender.sendMessage` -> `horizon_bukkit::CommandSender::send_component` `fn send_component(&self, text: &Component)`
- `ConfigurationSection.getItemStack` -> `horizon_bukkit::config::YamlConfiguration::get_item_stack` `fn get_item_stack(&self, path: &str) -> Option<ItemStack>`  (kit: yaml::{string,list_or_one,int,is_section,keys})
- `ConfigurationSection.getName` -> `horizon_bukkit::fam_core::SectionRef::get_name` `fn get_name(&self) -> &'a str`  (kit: yaml::{string,list_or_one,int,is_section,keys})
- `ConfigurationSection.getString` -> `horizon_bukkit::config::YamlConfiguration::get_string` `fn get_string(&self, path: &str) -> Option<String>`  (kit: yaml::{string,list_or_one,int,is_section,keys})
- `FileConfiguration.getBoolean` -> `horizon_bukkit::config::YamlConfiguration::get_boolean` `fn get_boolean(&self, path: &str) -> bool`  (kit: Config / yaml)
- `FileConfiguration.getInt` -> `horizon_bukkit::config::YamlConfiguration::get_int` `fn get_int(&self, path: &str) -> i32`  (kit: Config / yaml)
- `FileConfiguration.getString` -> `horizon_bukkit::config::YamlConfiguration::get_string` `fn get_string(&self, path: &str) -> Option<String>`  (kit: Config / yaml)
- `Inventory.getHolder` -> `horizon_bukkit::Inventory::get_holder` `fn get_holder(&self) -> Option<&Player>`  (kit: gui::Menu (buttons, pages, click routing))
- `Inventory.getItem` -> `horizon_bukkit::Inventory::get_item` `fn get_item(&self, slot: usize) -> Option<&ItemStack>`  (kit: gui::Menu (buttons, pages, click routing))
- `InventoryClickEvent.getRawSlot` -> `horizon_bukkit::fam_ev_player::InventoryClickEvent::get_raw_slot` `fn get_raw_slot(&self) -> i32`  (kit: gui::install routes menu clicks)
- `InventoryCloseEvent.getPlayer` -> `horizon_bukkit::fam_ev_player::InventoryCloseEvent::get_player` `fn get_player(&self) -> Option<Player>`
- `InventoryDragEvent.getType` -> `horizon_bukkit::fam_ev_player::InventoryDragEvent::get_type` `fn get_type(&self) -> &'static str`
- `ItemMeta.setMaxStackSize` -> `horizon_bukkit::ItemStack::set_max_stack_size` `fn set_max_stack_size(&mut self, value: i32)`
- `ItemStack.getPersistentDataContainer` -> `horizon_bukkit::ItemStack::edit_persistent_data_container` `fn edit_persistent_data_container(&mut self, edit: impl FnOnce(&mut Self)) -> bool`  (kit: item::Spec (from config), item::to_base64)
- `ItemStack.setData` -> `horizon_bukkit::ItemStack::set_data` `fn set_data(&mut self, component: &str, value: Value)`  (kit: item::Spec (from config), item::to_base64)
- `Location.getBlock` -> `horizon_bukkit::Location::get_block` `fn get_block(&self) -> Block`
- `Location.getBlockX` -> `horizon_bukkit::Location::get_block_x` `fn get_block_x(&self) -> i32`
- `Location.getBlockZ` -> `horizon_bukkit::Location::get_block_z` `fn get_block_z(&self) -> i32`
- `Location.set` -> `horizon_bukkit::Location::set` `fn set(&mut self, x: f64, y: f64, z: f64) -> &mut Self`
- `Material.isAir` -> `horizon_bukkit::Material::is_air` `fn is_air(&self) -> bool`
- `PersistentDataContainer.get` -> `horizon_bukkit::ItemStack::pdc_get` `fn pdc_get(&self, key: &NamespacedKey) -> Option<Value>`
- `PersistentDataContainer.set` -> `horizon_bukkit::ItemStack::pdc_set` `fn pdc_set(&mut self, key: &NamespacedKey, value: Value)`
- `Player.closeInventory` -> `horizon_bukkit::Player::close_inventory` `fn close_inventory(&self)`
- `Player.getName` -> `horizon_bukkit::Player::get_name` `fn get_name(&self) -> &str`
- `Player.getWorld` -> `horizon_bukkit::Player::get_world` `fn get_world(&self) -> World`
- `PlayerInteractEvent.getHand` -> `horizon_bukkit::fam_ev_player::PlayerInteractEvent::get_hand` `fn get_hand(&self) -> Option<&str>`
- `PlayerInteractEvent.setCancelled` -> `horizon_bukkit::fam_ev_player::PlayerInteractEvent::set_cancelled` `fn set_cancelled(&mut self, cancelled: bool)`
- `PlayerInteractEvent.setUseInteractedBlock` -> `horizon_bukkit::fam_ev_player::PlayerInteractEvent::set_use_interacted_block` `fn set_use_interacted_block(&mut self, result: &str)`
- `PlayerInteractEvent.setUseItemInHand` -> `horizon_bukkit::fam_ev_player::PlayerInteractEvent::set_use_item_in_hand` `fn set_use_item_in_hand(&mut self, result: &str)`
- `YamlConfiguration.options` -> `horizon_bukkit::fam_core::options` `fn options(&mut self) -> BoundOptions<'_>`  (kit: Config::load / yaml)
- `Block.getBlockData` -> `horizon_bukkit::Block::get_block_data` `fn get_block_data(&self) -> String`
- `Block.getState` -> `horizon_bukkit::Block::get_state` `fn get_state(&self) -> BlockState`
- `Block.getType` -> `horizon_bukkit::Block::get_type` `fn get_type(&self) -> Material`
- `Block.setBlockData` -> `horizon_bukkit::Block::set_block_data` `fn set_block_data(&self, data: &str) -> bool`
- `BlockBreakEvent.getPlayer` -> `horizon_bukkit::fam_ev_world::BlockBreakEvent::get_player` `fn get_player(&self) -> &Player`
- `BlockBreakEvent.setCancelled` -> `horizon_bukkit::fam_ev_world::BlockBreakEvent::set_cancelled` `fn set_cancelled(&mut self, cancelled: bool)`
- `BlockBreakEvent.setDropItems` -> `horizon_bukkit::fam_ev_world::BlockBreakEvent::set_drop_items` `fn set_drop_items(&mut self, drop: bool)`
- `BlockExplodeEvent.blockList` -> `horizon_bukkit::fam_ev_world::BlockExplodeEvent::block_list` `fn block_list(&mut self) -> &mut Vec<Block>`
- `BlockExplodeEvent.setCancelled` -> `horizon_bukkit::fam_ev_world::BlockExplodeEvent::set_cancelled` `fn set_cancelled(&mut self, cancelled: bool)`
- `BlockPlaceEvent.getBlockPlaced` -> `horizon_bukkit::fam_ev_world::BlockPlaceEvent::get_block_placed` `fn get_block_placed(&self) -> &Block`
- `BlockPlaceEvent.getItemInHand` -> `horizon_bukkit::fam_ev_world::BlockPlaceEvent::get_item_in_hand` `fn get_item_in_hand(&self) -> Option<&ItemStack>`
- `BlockPlaceEvent.getPlayer` -> `horizon_bukkit::fam_ev_world::BlockPlaceEvent::get_player` `fn get_player(&self) -> &Player`
- `BlockPlaceEvent.setCancelled` -> `horizon_bukkit::fam_ev_world::BlockPlaceEvent::set_cancelled` `fn set_cancelled(&mut self, cancelled: bool)`
- `BlockState.getBlock` -> `horizon_bukkit::BlockState::get_block` `fn get_block(&self) -> &Block`
- `BlockState.getBlockData` -> `horizon_bukkit::BlockState::get_block_data` `fn get_block_data(&self) -> &str`
- `BlockState.getLocation` -> `horizon_bukkit::block_data::TileState::get_location` `fn get_location(&self) -> Location`
- `BlockState.getType` -> `horizon_bukkit::BlockState::get_type` `fn get_type(&self) -> Material`
- `BlockState.getWorld` -> `horizon_bukkit::block_data::TileState::get_world` `fn get_world(&self) -> crate::World`
- `BlockState.setBlockData` -> `horizon_bukkit::BlockState::set_block_data` `fn set_block_data(&mut self, data: &str)`
- `BlockState.setType` -> `horizon_bukkit::world::set_type` `fn set_type(&mut self, material: &Material)`
- `BlockState.update` -> `horizon_bukkit::BlockState::update` `fn update(&self) -> bool`
- `Bukkit.createPlayerProfile` -> `horizon_bukkit::extra::create_player_profile` `fn create_player_profile(id: &str, name: &str) -> PlayerProfile`
- `Bukkit.getOnlinePlayers` -> `horizon_bukkit::get_online_players` `fn get_online_players() -> Vec<Player>`
- `Bukkit.getPlayerExact` -> `horizon_bukkit::get_player` `fn get_player(name: &str) -> Option<Player>`
- `Bukkit.getPluginManager` -> `horizon_bukkit::fam_core::get_plugin_manager` `fn get_plugin_manager() -> PluginManager`
- `Command.getName` -> `horizon_bukkit::pure2::Command::get_name` `fn get_name(&self) -> &str`
- `CommandSender.getName` -> `horizon_bukkit::CommandSender::get_name` `fn get_name(&self) -> &str`
- `ConfigurationSection.getLong` -> `horizon_bukkit::config::YamlConfiguration::get_long` `fn get_long(&self, path: &str) -> i64`  (kit: yaml::{string,list_or_one,int,is_section,keys})
- `EntityExplodeEvent.blockList` -> `horizon_bukkit::fam_ev_entity::EntityExplodeEvent::block_list` `fn block_list(&mut self) -> &mut Vec<Block>`
- `EntityExplodeEvent.getLocation` -> `horizon_bukkit::fam_ev_entity::EntityExplodeEvent::get_location` `fn get_location(&self) -> Option<Location>`
- `EntityExplodeEvent.setCancelled` -> `horizon_bukkit::fam_ev_entity::EntityExplodeEvent::set_cancelled` `fn set_cancelled(&mut self, cancelled: bool)`
- `FileConfiguration.getStringList` -> `horizon_bukkit::config::YamlConfiguration::get_string_list` `fn get_string_list(&self, path: &str) -> Vec<String>`  (kit: Config / yaml)
- `Inventory.getSize` -> `horizon_bukkit::Inventory::get_size` `fn get_size(&self) -> usize`  (kit: gui::Menu (buttons, pages, click routing))
- `Inventory.getViewers` -> `horizon_bukkit::Inventory::get_viewers` `fn get_viewers(&self) -> Vec<Player>`  (kit: gui::Menu (buttons, pages, click routing))
- `InventoryClickEvent.getCursor` -> `horizon_bukkit::fam_ev_player::InventoryClickEvent::get_cursor` `fn get_cursor(&self) -> Option<ItemStack>`  (kit: gui::install routes menu clicks)
- `InventoryClickEvent.isShiftClick` -> `horizon_bukkit::fam_ev_player::InventoryClickEvent::is_shift_click` `fn is_shift_click(&self) -> bool`  (kit: gui::install routes menu clicks)
- `InventoryDragEvent.getCursor` -> `horizon_bukkit::fam_ev_player::InventoryDragEvent::get_cursor` `fn get_cursor(&self) -> Option<ItemStack>`
- `InventoryDragEvent.getRawSlots` -> `horizon_bukkit::fam_ev_player::InventoryDragEvent::get_raw_slots` `fn get_raw_slots(&self) -> Vec<i32>`
- `InventoryMoveItemEvent.getDestination` -> `horizon_bukkit::fam_ev_player::InventoryMoveItemEvent::get_destination` `fn get_destination(&self) -> Option<MenuInventory>`
- `InventoryMoveItemEvent.getSource` -> `horizon_bukkit::fam_ev_player::InventoryMoveItemEvent::get_source` `fn get_source(&self) -> Option<MenuInventory>`
- `InventoryMoveItemEvent.setCancelled` -> `horizon_bukkit::fam_ev_player::InventoryMoveItemEvent::set_cancelled` `fn set_cancelled(&mut self, cancelled: bool)`
- `ItemStack.getAmount` -> `horizon_bukkit::ItemStack::get_amount` `fn get_amount(&self) -> i32`  (kit: item::Spec (from config), item::to_base64)
- `ItemStack.getMaxStackSize` -> `horizon_bukkit::ItemStack::get_max_stack_size` `fn get_max_stack_size(&self) -> i32`  (kit: item::Spec (from config), item::to_base64)
- `ItemStack.isSimilar` -> `horizon_bukkit::ItemStack::is_similar` `fn is_similar(&self, other: &Self) -> bool`  (kit: item::Spec (from config), item::to_base64)
- `ItemStack.of` -> `horizon_bukkit::ItemStack::new` `fn new(material: Material, amount: i32) -> Self`  (kit: item::Spec (from config), item::to_base64)
- `ItemStack.setAmount` -> `horizon_bukkit::ItemStack::set_amount` `fn set_amount(&mut self, amount: i32)`  (kit: item::Spec (from config), item::to_base64)
- `ItemStack.setType` -> `horizon_bukkit::ItemStack::set_type` `fn set_type(&mut self, material: Material)`  (kit: item::Spec (from config), item::to_base64)
- `Location.add` -> `horizon_bukkit::Location::add` `fn add(&mut self, x: f64, y: f64, z: f64) -> &mut Self`
- `Location.getBlockY` -> `horizon_bukkit::Location::get_block_y` `fn get_block_y(&self) -> i32`
- `Location.isChunkLoaded` -> `horizon_bukkit::Location::is_chunk_loaded` `fn is_chunk_loaded(&self) -> bool`
- `NamespacedKey.fromString` -> `horizon_bukkit::persistence::NamespacedKey::from_string` `fn from_string(text: &str) -> Option<Self>`
- `PersistentDataContainer.remove` -> `horizon_bukkit::ItemStack::pdc_remove` `fn pdc_remove(&mut self, key: &NamespacedKey)`
- `Player.getGameMode` -> `horizon_bukkit::Player::get_game_mode` `fn get_game_mode(&self) -> &'static str`
- `Player.getLocation` -> `horizon_bukkit::Player::get_location` `fn get_location(&self) -> Location`
- `Player.isSneaking` -> `horizon_bukkit::Player::is_sneaking` `fn is_sneaking(&self) -> bool`
- `PlayerInteractEvent.getClickedBlock` -> `horizon_bukkit::fam_ev_player::PlayerInteractEvent::get_clicked_block` `fn get_clicked_block(&self) -> Option<&Block>`
- `PlayerProfile.getTextures` -> `horizon_bukkit::fam_core::get_textures` `fn get_textures(&self) -> Self`
- `PluginCommand.setExecutor` -> `horizon_bukkit::Plugin::set_executor` `fn set_executor(&self, command: &str, executor: impl Fn(&CommandSender, &str, &[&str]) -> bool + Send + Sync + 'static)`
- `PluginCommand.setTabCompleter` -> `horizon_bukkit::Plugin::set_tab_completer` `fn set_tab_completer(&self, command: &str, completer: impl Fn(&CommandSender, &str, &[&str]) -> Vec<String> + Send + Sync + 'static)`
- `RegisteredServiceProvider.getPlugin` -> `horizon_bukkit::persistence::ServiceProvider::get_plugin` `fn get_plugin(&self) -> Option<crate::fam_core::LoadedPlugin>`
- `RegisteredServiceProvider.getProvider` -> `horizon_bukkit::persistence::ServiceProvider::call` `fn call(&self, method: &str, args: &horizon_mod::Value) -> horizon_mod::Value`
- `SkullMeta.setOwnerProfile` -> `horizon_bukkit::fam_items::set_owner_profile` `fn set_owner_profile(&mut self, profile: Option<&crate::player::PlayerProfile>)`
- `Statistic.valueOf` -> `horizon_bukkit::enums::statistic::value_of` `fn value_of(name: &str) -> Option<&'static str>`
- `TileState.getPersistentDataContainer` -> `horizon_bukkit::block_data::TileState::get_persistent_data_container` `fn get_persistent_data_container(&mut self) -> TileData<'_>`
- `Type.getType` -> `horizon_bukkit::fam_entity::ocelot_type_by_id` `fn ocelot_type_by_id(id: i32) -> Option<&'static str>`
- `World.getName` -> `horizon_bukkit::World::get_name` `fn get_name(&self) -> &str`
- `World.save` -> `horizon_bukkit::World::save` `fn save(&self)`

## Bukkit classes it imports that the SDK has no rows for (look these up, or work around)

- CommandExecutor, DataComponentTypes, InventoryHolder, Key, PersistentDataType, Placeholder, PlainTextComponentSerializer, TabCompleter, TagResolver

## Keys read from config / messages

- `chest.item.material`
- `chest.item.model-key`
- `item.material`
- `item.model-key`
- `item.texture`
- `messages.yml`
- `owner`
- `sounds.open`
- `sounds.upgrade-success`

## Biggest classes (write these first, they are most of the work)

- 11 KB com/zenithbackpacks/gui/ChestPagesGui.java
- 10 KB com/zenithbackpacks/listener/ChestGuiListener.java
- 6 KB com/zenithbackpacks/item/BackpackItemFactory.java
- 6 KB com/zenithbackpacks/command/ZenithBackpacksAdminCommand.java
- 5 KB com/zenithbackpacks/chest/ChestStore.java
- 5 KB com/zenithbackpacks/gui/BackpackUpgradeGui.java
- 5 KB com/zenithbackpacks/ZenithBackpacksPlugin.java
- 5 KB com/zenithbackpacks/listener/ChestBlockListener.java
- 5 KB com/zenithbackpacks/chest/ChestItemFactory.java
- 4 KB com/zenithbackpacks/listener/BackpackGuiListener.java
- 4 KB com/zenithbackpacks/config/PluginConfig.java
- 4 KB com/zenithbackpacks/chest/PlacedChestService.java
- 4 KB com/zenithbackpacks/gui/ChestUpgradeGui.java
- 3 KB com/zenithbackpacks/storage/BackpackStore.java
- 3 KB com/zenithbackpacks/chest/ChestSortService.java
- 3 KB com/zenithbackpacks/chest/ChestConfig.java
- 3 KB com/zenithbackpacks/progression/RequirementEvaluator.java
- 3 KB com/zenithbackpacks/listener/BackpackInteractListener.java
- 3 KB com/zenithbackpacks/chest/ChatPrompt.java
- 3 KB com/zenithbackpacks/chest/PlacedChestIndex.java
- 2 KB com/zenithbackpacks/listener/ChestInteractListener.java
- 2 KB com/zenithbackpacks/chest/ChestData.java
- 2 KB com/zenithbackpacks/gui/BackpackStorageGui.java
- 1 KB com/zenithbackpacks/config/RequirementParser.java
- 1 KB com/zenithbackpacks/chest/ChestPageMath.java

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
