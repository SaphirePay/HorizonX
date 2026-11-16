# zAuctionHouse 4.0.1.6: rewrite brief

Main class `fr.maxlego08.zauctionhouse.ZAuctionPlugin`. 431 own classes, 1379 KB of Java (java/).

## Commands

- `/zauctionhouse`: zAuctionHouse (registered in its code; added here)

## Sub-commands found in the source

- `expired` (fr/maxlego08/zauctionhouse/command/commands/admin/CommandAuctionAdminAdd.java:71)
- `purchased` (fr/maxlego08/zauctionhouse/command/commands/admin/CommandAuctionAdminAdd.java:74)

## Permissions

- `zplugin.notifs`

## Events

- PlayerDeathEvent -> onDeath (fr/maxlego08/zauctionhouse/listeners/PlayerListener.java)
- PlayerJoinEvent -> onConnect (fr/maxlego08/zauctionhouse/listeners/PlayerListener.java)
- PlayerJoinEvent -> onConnect (fr/maxlego08/zauctionhouse/utils/VersionChecker.java)
- PlayerQuitEvent -> onQuit (fr/maxlego08/zauctionhouse/listeners/PlayerListener.java)
- PlayerQuitEvent -> onQuit (fr/maxlego08/zauctionhouse/search/ChatSearchListener.java)

## Timers


## Files

- data/categories.yml
- data/config.yml
- data/discord.yml
- data/economies.yml
- data/language.yml
- data/messages.yml
- data/rules.yml
- data/dialogs/search.yml
- data/es/categories.yml
- data/es/config.yml
- data/es/discord.yml
- data/es/economies.yml
- data/es/messages.yml
- data/es/rules.yml
- data/es/dialogs/search.yml
- data/es/inventories/auction.yml
- data/es/inventories/categories.yml
- data/es/inventories/combined-items.yml
- data/es/inventories/expired-items.yml
- data/es/inventories/history.yml
- data/es/inventories/options.yml
- data/es/inventories/purchased-items.yml
- data/es/inventories/sell-inventory.yml
- data/es/inventories/selling-items.yml
- data/es/inventories/shulker-content.yml
- data/es/patterns/back.yml
- data/es/patterns/decoration.yml
- data/es/patterns/pagination.yml
- data/fr/categories.yml
- data/fr/config.yml
- data/fr/discord.yml
- data/fr/economies.yml
- data/fr/messages.yml
- data/fr/rules.yml
- data/fr/dialogs/search.yml
- data/fr/inventories/auction.yml
- data/fr/inventories/categories.yml
- data/fr/inventories/combined-items.yml
- data/fr/inventories/expired-items.yml
- data/fr/inventories/history.yml
- data/fr/inventories/options.yml
- data/fr/inventories/purchased-items.yml
- data/fr/inventories/sell-inventory.yml
- data/fr/inventories/selling-items.yml
- data/fr/inventories/shulker-content.yml
- data/fr/patterns/back.yml
- data/fr/patterns/decoration.yml
- data/fr/patterns/pagination.yml
- data/id/categories.yml
- data/id/config.yml
- data/id/discord.yml
- data/id/economies.yml
- data/id/messages.yml
- data/id/rules.yml
- data/id/dialogs/search.yml
- data/id/inventories/auction.yml
- data/id/inventories/categories.yml
- data/id/inventories/combined-items.yml
- data/id/inventories/expired-items.yml
- data/id/inventories/history.yml
- data/id/inventories/options.yml
- data/id/inventories/purchased-items.yml
- data/id/inventories/sell-inventory.yml
- data/id/inventories/selling-items.yml
- data/id/inventories/shulker-content.yml
- data/id/patterns/back.yml
- data/id/patterns/decoration.yml
- data/id/patterns/pagination.yml
- data/inventories/auction.yml
- data/inventories/categories.yml
- data/inventories/combined-items.yml
- data/inventories/expired-items.yml
- data/inventories/history.yml
- data/inventories/options.yml
- data/inventories/purchased-items.yml
- data/inventories/sell-inventory.yml
- data/inventories/selling-items.yml
- data/inventories/shulker-content.yml
- data/inventories/admin/admin-expired-items.yml
- data/inventories/admin/admin-history-main.yml
- data/inventories/admin/admin-logs.yml
- data/inventories/admin/admin-main.yml
- data/inventories/admin/admin-purchased-items.yml
- data/inventories/admin/admin-remove-confirm.yml
- data/inventories/admin/admin-selling-items.yml
- data/inventories/admin/admin-transactions.yml
- data/inventories/confirms/purchase-confirm.yml
- data/inventories/confirms/purchase-inventory-confirm.yml
- data/inventories/confirms/remove-confirm.yml
- data/inventories/confirms/remove-inventory-confirm.yml
- data/it/categories.yml
- data/it/config.yml
- data/it/discord.yml
- data/it/economies.yml
- data/it/messages.yml
- data/it/rules.yml
- data/it/dialogs/search.yml
- data/it/inventories/auction.yml
- data/it/inventories/categories.yml
- data/it/inventories/combined-items.yml
- data/it/inventories/expired-items.yml
- data/it/inventories/history.yml
- data/it/inventories/options.yml
- data/it/inventories/purchased-items.yml
- data/it/inventories/sell-inventory.yml
- data/it/inventories/selling-items.yml
- data/it/inventories/shulker-content.yml
- data/it/patterns/back.yml
- data/it/patterns/decoration.yml
- data/it/patterns/pagination.yml
- data/patterns/back.yml
- data/patterns/decoration.yml
- data/patterns/pagination.yml
- data/th/categories.yml
- data/th/config.yml
- data/th/discord.yml
- data/th/economies.yml
- data/th/messages.yml
- data/th/rules.yml
- data/th/dialogs/search.yml
- data/th/inventories/auction.yml
- data/th/inventories/categories.yml
- data/th/inventories/combined-items.yml
- data/th/inventories/expired-items.yml
- data/th/inventories/history.yml
- data/th/inventories/options.yml
- data/th/inventories/purchased-items.yml
- data/th/inventories/sell-inventory.yml
- data/th/inventories/selling-items.yml
- data/th/inventories/shulker-content.yml
- data/th/patterns/back.yml
- data/th/patterns/decoration.yml
- data/th/patterns/pagination.yml
- data/currenciesapi.properties

## Keys read from config / messages

- `Database-Type`
- `MMOITEMS_ITEM_ID`
- `MySQL.database`
- `MySQL.host`
- `MySQL.password`
- `MySQL.user`
- `action.container-preview.click`
- `after_action`
- `all-category-name`
- `amount-type`
- `auction_type`
- `auto-claim.deposit-reason`
- `buyer`
- `bypass-permission`
- `cancel-label`
- `category`
- `clear-label`
- `color`
- `commands.cooldown.bypass-permission`
- `currency-name`
- `database`
- `database-configuration.database`
- `database-configuration.host`
- `database-configuration.password`
- `database-configuration.table-prefix`
- `database-configuration.user`
- `date-format`
- `default-color`
- `default-economy.`
- `default-value`
- `deposit-reason`
- `description`
- `display`
- `display-name`
- `economy`
- `format`
- `fromUUID`
- `fromUsername`
- `host`
- `id`
- `inventory`
- `item`
- `item-display.and`
- `item-display.between`
- `item-display.item-name`
- `item-display.lang`
- `item-image-url`
- `item-lore.status.buyer`
- `item-lore.status.right-buyer`
- `item-lore.status.right-seller`
- `item-lore.status.seller`
- `itemstack`
- `json-folder`
- `language`
- `max-amount`
- `messages.yml`
- `multiplication`
- `name`
- `overlay`
- `pageName`
- `password`
- `performance-debug.filter.mode`
- `permission`
- `price-decimal-format`
- `price-format`
- `query-label`
- `search-filters.`
- `search-label`
- `seller`
- `sellerName`
- `server-name`
- `serverUuid`
- `server_name`
- `sort-items.default-sort`
- `sort-items.translations.`
- `source-type`
- `sqlite-path`
- `storage-type`
- `storage_type`
- `symbol`
- `table-prefix`
- `timezone`
- `toUsername`
- `transactionType`
- `type`
- `unique_id`
- `user`
- `withdraw-reason`

## Biggest classes (write these first, they are most of the work)

- 82 KB fr/maxlego08/zauctionhouse/ZAuctionManager.java
- 37 KB fr/maxlego08/zauctionhouse/services/SellService.java
- 29 KB fr/maxlego08/zauctionhouse/ZAuctionPlugin.java
- 27 KB fr/maxlego08/zauctionhouse/services/ClaimService.java
- 26 KB fr/maxlego08/zauctionhouse/economy/ZEconomyManager.java
- 26 KB fr/maxlego08/zauctionhouse/services/RemoveService.java
- 22 KB fr/maxlego08/zauctionhouse/services/ExpireService.java
- 19 KB fr/maxlego08/zauctionhouse/utils/Metrics.java
- 19 KB fr/maxlego08/zauctionhouse/storage/ZStorageManager.java
- 18 KB fr/maxlego08/zauctionhouse/services/PurchaseService.java
- 17 KB fr/maxlego08/zauctionhouse/loader/ZInventoriesLoader.java
- 17 KB fr/maxlego08/zauctionhouse/hooks/zelauction/ZelAuctionMigrationService.java
- 16 KB fr/maxlego08/zauctionhouse/command/commands/admin/CommandAuctionAdminGenerate.java
- 16 KB fr/maxlego08/zauctionhouse/utils/yaml/YamlUpdater.java
- 16 KB fr/maxlego08/zauctionhouse/migration/v3/V3MigrationService.java
- 16 KB fr/maxlego08/zauctionhouse/api/messages/Message.java
- 15 KB fr/maxlego08/zauctionhouse/buttons/list/ListedItemsButton.java
- 15 KB fr/maxlego08/zauctionhouse/utils/cache/SortedItemsCache.java
- 14 KB fr/maxlego08/zauctionhouse/configuration/MainConfiguration.java
- 13 KB fr/maxlego08/zauctionhouse/loader/MessageLoader.java
- 12 KB fr/maxlego08/zauctionhouse/hooks/crazyauctions/CrazyAuctionsMigrationService.java
- 12 KB fr/maxlego08/zauctionhouse/storage/repository/repositories/ItemRepository.java
- 12 KB fr/maxlego08/zauctionhouse/economy/ZAuctionEconomy.java
- 11 KB fr/maxlego08/zauctionhouse/command/commands/admin/CommandAuctionAdminMigrate.java
- 11 KB fr/maxlego08/zauctionhouse/api/command/VCommand.java

## Kit map

- messages: `msg("key", &[args])` (Lang: {prefix}, %s, {0}); settings: `config(|c| c.int("path", default))`
- commands: the Router above; a sub = `fn(&Ctx) -> Res` (`c.player()?`, `c.int(0, "amount")?`, `c.online(0)?`, `c.tell(..)`)
- client-only entities (holograms, markers, NPCs): `horizon_kit::packets` (+ `wire` for raw bytes)
- per-tick work: `horizon_kit::worker` (server-thread cost ~0, as Java async)
- a `/x profiler`: `horizon_kit::stats::{set_running, report, timed}`
- any Bukkit call: `mcp plugin-port api_lookup <Class.method>` / `sdk_search`
