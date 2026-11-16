# ChestShop 3.12.2 (build 353): rewrite brief

Main class `com.Acrobot.ChestShop.ChestShop`. 130 own classes, 390 KB of Java (java/).

## Commands

- `/iteminfo` aliases ['iinfo']: Lists item id and names
- `/shopinfo` aliases ['sinfo', 'shop']: Shows information about the shop looked at
- `/csGive`: Gives an item to the appropriate player
- `/csVersion` aliases ['chestshop']: Shows the ChestShop's version
- `/csMetrics`: Shows ChestShop's metrics
- `/cstoggle`: Toggle messages to the owner of a shop
- `/csaccess`: Allows trading at shops that you have access to

## Sub-commands found in the source

- `reload` (com/Acrobot/ChestShop/Commands/Version.java:12)

## Permissions

- `ChestShop.*`
- `ChestShop.accesstoggle`
- `ChestShop.admin`
- `ChestShop.adminshop`
- `ChestShop.iteminfo`
- `ChestShop.mod`
- `ChestShop.name.(some name)`
- `ChestShop.name.*`
- `ChestShop.nofee`
- `ChestShop.notax.buy`
- `ChestShop.notax.sell`
- `ChestShop.othername.access.(some name)`
- `ChestShop.othername.access.*`
- `ChestShop.othername.create.(some name)`
- `ChestShop.othername.create.*`
- `ChestShop.othername.destroy.(some name)`
- `ChestShop.othername.destroy.*`
- `ChestShop.shop.*`
- `ChestShop.shop.buy`
- `ChestShop.shop.buy.(itemType)`
- `ChestShop.shop.create`
- `ChestShop.shop.create.(itemType)`
- `ChestShop.shop.create.banners`
- `ChestShop.shop.create.basic`
- `ChestShop.shop.create.beds`
- `ChestShop.shop.create.bench`
- `ChestShop.shop.create.boats`
- `ChestShop.shop.create.bows`
- `ChestShop.shop.create.brewing`
- `ChestShop.shop.create.buy`
- `ChestShop.shop.create.buy.(itemType)`
- `ChestShop.shop.create.chainarmor`
- `ChestShop.shop.create.diamondarmor`
- `ChestShop.shop.create.diamondgrade`
- `ChestShop.shop.create.diamondtools`
- `ChestShop.shop.create.doors`
- `ChestShop.shop.create.end`
- `ChestShop.shop.create.fences`
- `ChestShop.shop.create.food`
- `ChestShop.shop.create.goldarmor`
- `ChestShop.shop.create.goldgrade`
- `ChestShop.shop.create.goldtools`
- `ChestShop.shop.create.ingots`
- `ChestShop.shop.create.ironarmor`
- `ChestShop.shop.create.irongrade`
- `ChestShop.shop.create.irontools`
- `ChestShop.shop.create.leatherarmor`
- `ChestShop.shop.create.misc`
- `ChestShop.shop.create.misctools`
- `ChestShop.shop.create.monsterdrops`
- `ChestShop.shop.create.music`
- `ChestShop.shop.create.netherblocks`
- `ChestShop.shop.create.netherdrops`
- `ChestShop.shop.create.ore`
- `ChestShop.shop.create.plants`
- `ChestShop.shop.create.redstoneitems`
- `ChestShop.shop.create.sell`
- `ChestShop.shop.create.sell.(itemType)`
- `ChestShop.shop.create.shulkerbox`
- `ChestShop.shop.create.spawn_eggs`
- `ChestShop.shop.create.stairs`
- `ChestShop.shop.create.steps`
- `ChestShop.shop.create.stonegrade`
- `ChestShop.shop.create.stonetools`
- `ChestShop.shop.create.unobtainables`
- `ChestShop.shop.create.vehicles`
- `ChestShop.shop.create.wall_banners`
- `ChestShop.shop.create.woodgrade`
- `ChestShop.shop.create.woodtools`
- `ChestShop.shop.create.woolitems`
- `ChestShop.shop.sell`
- `ChestShop.shop.sell.(itemType)`
- `ChestShop.shopinfo`
- `ChestShop.toggle`

## Events

- ChestShopReloadEvent -> onReload (com/Acrobot/ChestShop/Listeners/Modules/DiscountModule.java) **no SDK event of this name**
- ChestShopReloadEvent -> onReload (com/Acrobot/ChestShop/Listeners/Modules/ItemAliasModule.java) **no SDK event of this name**
- ChestShopReloadEvent -> onReload (com/Acrobot/ChestShop/Listeners/Modules/PriceRestrictionModule.java) **no SDK event of this name**
- PreShopCreationEvent -> onPreShopCreation (com/Acrobot/ChestShop/Listeners/Modules/PriceRestrictionModule.java) **no SDK event of this name**
- ProtectBlockEvent -> onBlockProtect (com/Acrobot/ChestShop/Plugins/LightweightChestProtection.java) **no SDK event of this name**
- ProtectionCheckEvent -> onProtectionCheck (com/Acrobot/ChestShop/Plugins/WorldGuardProtection.java) **no SDK event of this name**
- ServiceRegisterEvent -> onServiceRegister (com/Acrobot/ChestShop/Listeners/Economy/Plugins/VaultListener.java)
- ServiceUnregisterEvent -> onServiceUnregister (com/Acrobot/ChestShop/Listeners/Economy/Plugins/VaultListener.java)
- ShopCreatedEvent -> shopCreated (com/Acrobot/ChestShop/Plugins/Heroes.java) **no SDK event of this name**
- ShopDestroyedEvent -> onShopRemove (com/Acrobot/ChestShop/Plugins/LightweightChestProtection.java) **no SDK event of this name**

## Timers


## Files

- data/languages/lang.uk.yml
- data/languages/lang.en.yml
- data/languages/lang.fi.yml
- data/languages/lang.de.yml
- data/languages/lang.zh.yml
- data/languages/lang.ca.yml
- data/languages/lang.ru.yml
- data/languages/lang.he.yml
- data/languages/lang.ro.yml
- data/languages/lang.vi.yml
- data/languages/lang.no.yml
- data/languages/lang.ko.yml
- data/languages/lang.af.yml
- data/languages/lang.zh_TW.yml
- data/languages/lang.nl.yml
- data/languages/lang.tr.yml
- data/languages/lang.sr.yml
- data/languages/lang.es_MX.yml
- data/languages/lang.da.yml
- data/languages/lang.sv.yml
- data/languages/lang.ja.yml
- data/languages/lang.pt_BR.yml
- data/languages/lang.cs.yml
- data/languages/lang.pl.yml
- data/languages/lang.fr.yml
- data/languages/lang.hu.yml
- data/languages/lang.el.yml
- data/languages/lang.es.yml
- data/languages/lang.it.yml
- data/languages/lang.ar.yml

## Keys read from config / messages

- `api-key`
- `jenkins-job-url-override`

## Biggest classes (write these first, they are most of the work)

- 24 KB com/Acrobot/ChestShop/ChestShop.java
- 15 KB com/Acrobot/ChestShop/Configuration/Properties.java
- 14 KB com/Acrobot/ChestShop/Updater/Updater.java
- 13 KB com/Acrobot/ChestShop/Listeners/Player/PlayerInteract.java
- 12 KB com/Acrobot/ChestShop/Listeners/PreTransaction/PartialTransactionModule.java
- 12 KB com/Acrobot/ChestShop/UUIDs/NameManager.java
- 11 KB com/Acrobot/ChestShop/Listeners/Economy/Plugins/VaultListener.java
- 9 KB com/Acrobot/ChestShop/Utils/uBlock.java
- 9 KB com/Acrobot/ChestShop/Configuration/Messages.java
- 8 KB com/Acrobot/ChestShop/Plugins/LightweightChestProtection.java
- 8 KB com/Acrobot/ChestShop/Metadata/ItemDatabase.java
- 7 KB com/Acrobot/ChestShop/Signs/ChestShopSign.java
- 6 KB com/Acrobot/ChestShop/Listeners/PreTransaction/ErrorMessageSender.java
- 6 KB com/Acrobot/ChestShop/Listeners/Modules/PriceRestrictionModule.java
- 6 KB com/Acrobot/ChestShop/Listeners/Block/Break/SignBreak.java
- 6 KB com/Acrobot/ChestShop/Dependencies.java
- 6 KB com/Acrobot/ChestShop/Listeners/Modules/StockCounterModule.java
- 5 KB com/Acrobot/ChestShop/Database/Migrations.java
- 5 KB com/Acrobot/ChestShop/Containers/AdminInventory.java
- 5 KB com/Acrobot/ChestShop/Signs/RestrictedSign.java
- 4 KB com/Acrobot/ChestShop/Listeners/Economy/ServerAccountCorrector.java
- 4 KB com/Acrobot/ChestShop/Events/PreTransactionEvent.java
- 4 KB com/Acrobot/ChestShop/Listeners/Modules/ItemAliasModule.java
- 4 KB com/Acrobot/ChestShop/Updater/JenkinsBuildsNotifier.java
- 4 KB com/Acrobot/ChestShop/Listeners/Economy/Plugins/ReserveListener.java

## Kit map

- messages: `msg("key", &[args])` (Lang: {prefix}, %s, {0}); settings: `config(|c| c.int("path", default))`
- commands: the Router above; a sub = `fn(&Ctx) -> Res` (`c.player()?`, `c.int(0, "amount")?`, `c.online(0)?`, `c.tell(..)`)
- client-only entities (holograms, markers, NPCs): `horizon_kit::packets` (+ `wire` for raw bytes)
- per-tick work: `horizon_kit::worker` (server-thread cost ~0, as Java async)
- a `/x profiler`: `horizon_kit::stats::{set_running, report, timed}`
- any Bukkit call: `mcp plugin-port api_lookup <Class.method>` / `sdk_search`
