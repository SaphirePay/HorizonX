# Jobs 5.2.6.3: rewrite brief

Main class `com.gamingmesh.jobs.Jobs`. 235 own classes, 1389 KB of Java (java/).

## Commands

- `/jobs`: Jobs

## Sub-commands found in the source

- none found: read the command classes

## Permissions

- `jobs.*`
- `jobs.admin`
- `jobs.all.vipmaxlevel`
- `jobs.area.add`
- `jobs.area.remove`
- `jobs.area.select`
- `jobs.autojoin.*`
- `jobs.command.*`
- `jobs.command.admin.*`
- `jobs.command.admin.archive`
- `jobs.command.admin.clearownership`
- `jobs.command.admin.ownedblocks`
- `jobs.command.admin.stats`
- `jobs.command.archive`
- `jobs.command.blockinfo`
- `jobs.command.bonus`
- `jobs.command.browse`
- `jobs.command.clearownership`
- `jobs.command.demote`
- `jobs.command.edititembonus`
- `jobs.command.employ`
- `jobs.command.exp`
- `jobs.command.expboost`
- `jobs.command.fire`
- `jobs.command.fireall`
- `jobs.command.fixnames`
- `jobs.command.give`
- `jobs.command.glog`
- `jobs.command.grantxp`
- `jobs.command.gtop`
- `jobs.command.info`
- `jobs.command.itembonus`
- `jobs.command.iteminfo`
- `jobs.command.join`
- `jobs.command.leave`
- `jobs.command.leaveall`
- `jobs.command.level`
- `jobs.command.limit`
- `jobs.command.log`
- `jobs.command.log.others`
- `jobs.command.moneyboost`
- `jobs.command.ownedblocks`
- `jobs.command.playerinfo`
- `jobs.command.points`
- `jobs.command.promote`
- `jobs.command.quests`
- `jobs.command.reload`
- `jobs.command.removexp`
- `jobs.command.schedule`
- `jobs.command.shop`
- `jobs.command.signs`
- `jobs.command.signupdate`
- `jobs.command.skipquest`
- `jobs.command.stats`
- `jobs.command.toggle`
- `jobs.command.top`
- `jobs.command.transfer`
- `jobs.disabledworld.commands`
- `jobs.noownershiptakeover`
- `jobs.paycreative`
- `jobs.rejoinbypass`
- `jobs.tax.bypass`
- `jobs.use`
- `jobs.versioncheck`

## Events

- InventoryClickEvent -> playerItemBreakEvent (com/gamingmesh/jobs/listeners/JobsListener.java)
- JobsArmorChangeEvent -> jobsArmorChangeEvent (com/gamingmesh/jobs/listeners/JobsListener.java) **no SDK event of this name**
- McMMOPlayerRepairCheckEvent -> OnItemrepair (com/gamingmesh/jobs/hooks/McMMO/McMMO1_X_listener.java) **no SDK event of this name**
- McMMOPlayerRepairCheckEvent -> OnItemrepair (com/gamingmesh/jobs/hooks/McMMO/McMMO2_X_listener.java) **no SDK event of this name**
- MythicMobDeathEvent -> onMythicMobDeath (com/gamingmesh/jobs/hooks/MythicMobs/MythicMobs5Listener.java) **no SDK event of this name**
- PlayerInteractEvent -> playerInteractEvent (com/gamingmesh/jobs/listeners/JobsListener.java)
- PlayerItemBreakEvent -> playerItemBreakEvent (com/gamingmesh/jobs/listeners/JobsListener.java)
- PlayerItemHeldEvent -> playerItemHeldEvent (com/gamingmesh/jobs/listeners/JobsListener.java)
- PlayerSwapHandItemsEvent -> onPlayerHandSwap (com/gamingmesh/jobs/listeners/JobsPayment1_9Listener.java)

## Timers

- runTaskLater delay 200 period  (com/gamingmesh/jobs/container/JobsMobSpawner.java)
- runTaskLater delay 200 period  (com/gamingmesh/jobs/container/JobsMobSpawner.java)
- runTaskLater delay 40 period  (com/gamingmesh/jobs/listeners/JobsListener.java)
- runTaskLater delay 1 period  (com/gamingmesh/jobs/listeners/JobsListener.java)

## Files

- data/jobs/brewer.yml
- data/jobs/builder.yml
- data/jobs/crafter.yml
- data/jobs/digger.yml
- data/jobs/enchanter.yml
- data/jobs/explorer.yml
- data/jobs/farmer.yml
- data/jobs/fisherman.yml
- data/jobs/hunter.yml
- data/jobs/miner.yml
- data/jobs/none.yml
- data/jobs/weaponsmith.yml
- data/jobs/woodcutter.yml
- data/jobs/_EXAMPLE.yml
- data/locale/messages_bg.yml
- data/locale/messages_cs.yml
- data/locale/messages_de.yml
- data/locale/messages_es.yml
- data/locale/messages_fr.yml
- data/locale/messages_hu.yml
- data/locale/messages_it_IT.yml
- data/locale/messages_ja.yml
- data/locale/messages_ko.yml
- data/locale/messages_nl_NL.yml
- data/locale/messages_no_NO.yml
- data/locale/messages_pl.yml
- data/locale/messages_pt-BR.yml
- data/locale/messages_ro.yml
- data/locale/messages_ru.yml
- data/locale/messages_sv-SE.yml
- data/locale/messages_tr.yml
- data/locale/messages_uk.yml
- data/locale/messages_zhcn.yml
- data/locale/messages_zhtw.yml
- data/restrictedBlocks.yml
- data/schedule.yml
- data/shopItems.yml
- data/Signs.yml
- data/TranslatableWords/Words_de.yml
- data/TranslatableWords/Words_es.yml
- data/TranslatableWords/Words_fr.yml
- data/TranslatableWords/Words_ru.yml
- data/TranslatableWords/Words_zhtw.yml

## Keys read from config / messages

- `Action`
- `BossBarColour`
- `ChatColour`
- `CustomSkull`
- `Days`
- `EntityList.`
- `From`
- `FullDescription`
- `Icon.CustomHead.PlayerName`
- `Icon.Id`
- `Icon.ItemStack`
- `Icon.Name`
- `Id`
- `Item`
- `ItemList.`
- `ItemStack`
- `JobName`
- `Jobs`
- `JobsItemBoost`
- `JobsItemBoost.`
- `JobsLimitedNode`
- `Loc`
- `Name`
- `Target`
- `Type`
- `Until`
- `World`
- `chat-display`
- `command`
- `description`
- `displayName`
- `experience-progression-equation`
- `fullname`
- `id`
- `income-progression-equation`
- `leveling-progression-equation`
- `message.blocktimer`
- `message.boostStarted`
- `message.boostStoped`
- `message.cowtimer`
- `message.crafting.fullinventory`
- `message.leveldown.message`
- `message.levelup.`
- `message.levelup.broadcast`
- `message.levelup.nobroadcast`
- `message.skillup.`
- `message.skillup.broadcast`
- `message.skillup.nobroadcast`
- `message.taxes`
- `mysql-database`
- `mysql-hostname`
- `mysql-password`
- `mysql-table-prefix`
- `mysql-username`
- `mysql.url`
- `name`
- `permission`
- `points-progression-equation`
- `potion-type`
- `shortname`

## Biggest classes (write these first, they are most of the work)

- 113 KB com/gamingmesh/jobs/dao/JobsDAO.java
- 92 KB com/gamingmesh/jobs/config/GeneralConfigManager.java
- 87 KB com/gamingmesh/jobs/listeners/JobsPaymentListener.java
- 54 KB com/gamingmesh/jobs/config/ConfigManager.java
- 46 KB com/gamingmesh/jobs/Jobs.java
- 45 KB com/gamingmesh/jobs/config/LanguageManager.java
- 41 KB com/gamingmesh/jobs/container/JobsPlayer.java
- 38 KB com/gamingmesh/jobs/PlayerManager.java
- 33 KB com/gamingmesh/jobs/config/ExampleJob.java
- 32 KB com/gamingmesh/jobs/listeners/JobsListener.java
- 28 KB com/gamingmesh/jobs/Placeholders/Placeholder.java
- 28 KB com/gamingmesh/jobs/config/ShopManager.java
- 23 KB com/gamingmesh/jobs/commands/list/editquests.java
- 22 KB com/gamingmesh/jobs/container/blockOwnerShip/BlockOwnerShip.java
- 20 KB com/gamingmesh/jobs/Gui/GuiManager.java
- 20 KB com/gamingmesh/jobs/commands/list/editjobs.java
- 19 KB com/gamingmesh/jobs/container/Job.java
- 19 KB com/gamingmesh/jobs/config/NameTranslatorManager.java
- 18 KB com/gamingmesh/jobs/commands/JobsCommands.java
- 15 KB com/gamingmesh/jobs/Signs/SignUtil.java
- 14 KB com/gamingmesh/jobs/config/RestrictedAreaManager.java
- 13 KB com/gamingmesh/jobs/commands/list/quests.java
- 12 KB com/gamingmesh/jobs/config/ExploitProtectionManager.java
- 12 KB com/gamingmesh/jobs/commands/list/browse.java
- 11 KB com/gamingmesh/jobs/stuff/Util.java

## Kit map

- messages: `msg("key", &[args])` (Lang: {prefix}, %s, {0}); settings: `config(|c| c.int("path", default))`
- commands: the Router above; a sub = `fn(&Ctx) -> Res` (`c.player()?`, `c.int(0, "amount")?`, `c.online(0)?`, `c.tell(..)`)
- client-only entities (holograms, markers, NPCs): `horizon_kit::packets` (+ `wire` for raw bytes)
- per-tick work: `horizon_kit::worker` (server-thread cost ~0, as Java async)
- a `/x profiler`: `horizon_kit::stats::{set_running, report, timed}`
- any Bukkit call: `mcp plugin-port api_lookup <Class.method>` / `sdk_search`
