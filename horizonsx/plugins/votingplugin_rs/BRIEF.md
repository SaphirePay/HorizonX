# VotingPlugin 7.1.1: rewrite brief

Main class `com.bencodez.votingplugin.VotingPluginMain`. 578 own classes, 3665 KB of Java (java/).

## Commands

- `/vote`: Vote command
- `/adminvote`: Admin commands for VotingPlugin
- `/av`: Admin commands for VotingPlugin
- `/votehelp` aliases vhelp: See access to a list of commands, Formated in Format.yml
- `/votetotal` aliases vtotal: See totals of votes
- `/votelast` aliases vlast: See the last time you voted
- `/votenext` aliases vnext: See when you can vote next
- `/votetop` aliases vtop: See top voters
- `/votetoday` aliases vtoday: See who voted today
- `/votegui` aliases vgui: Open VoteGUI
- `/voteurl` aliases vurl: Open VoteURL
- `/voteparty` aliases vparty: A command
- `/voteshop` aliases vshop: A command
- `/votebest` aliases vbest: A command
- `/votestreak` aliases vstreak: A command
- `/votetogglebroadcast` aliases vtogglebroadcast: A command
- `/votepoints` aliases vpoints: A command
- `/votelist` aliases vlist: A command
- `/voteall` aliases vall: A command
- `/adminvotehelp` aliases avhelp: Open adminvote help
- `/adminvotereport` aliases avreport: Command Alias
- `/adminvoteperms` aliases avperms: Command Alias
- `/adminvotereload` aliases avreload: Command Alias
- `/adminvotetestreward` aliases avtestreward: Command Alias
- `/adminvoteversion` aliases avversion: Command Alias
- `/adminvotesites` aliases avsites: Command Alias
- `/adminvoterewards` aliases avrewards: Command Alias
- `/adminvoteuuid` aliases avuuid: Command Alias
- `/adminvotevotesite` aliases avvotesite: Command Alias
- `/adminvoteconfig` aliases avconfig: Command Alias
- `/adminvotevote` aliases ['avvote', 'fakevote']: Command Alias
- `/adminvotegui` aliases avgui: Command Alias
- `/adminvotegivereward` aliases avgivereward: Command Alias
- `/adminvotecleartotal`: Command Alias
- `/adminvotebackgroundupdate` aliases avbackgroundupdate: Command Alias
- `/adminvoteservicesites` aliases avservicesites: Command Alias
- `/adminvotevoteparty` aliases avvoteparty: Command Alias
- `/adminvotereloadall` aliases avreloadall: Command Alias
- `/adminvotepermsplayer` aliases avpermsplayer: Command Alias
- `/adminvoteclearofflinevotes` aliases avclearofflinevotes: Command Alias
- `/adminvotegiveall` aliases avgiveall: Command Alias
- `/adminvotegiveallonline` aliases avgiveallonline: Command Alias
- `/adminvoteplayername` aliases avplayername: Command Alias
- `/adminvotestatus` aliases avstatus: Command Alias
- `/adminvotetest` aliases avtest: Command Alias
- `/adminvoteplaceholders` aliases avplaceholders: Command Alias
- `/adminvoteuser` aliases avuser: Command Alias

## Sub-commands found in the source

- none found: read the command classes

## Permissions

- `AdvancedCore.UserEdit`
- `VotingPlugin.Admin`
- `VotingPlugin.Admin.GenerateServiceSite`
- `VotingPlugin.BypassWaitUntilVoteDelay`
- `VotingPlugin.Login.RemindVotes`
- `VotingPlugin.Login.RemindVotes.All`
- `VotingPlugin.Mod`
- `VotingPlugin.NoRemind`
- `VotingPlugin.Player`
- `VotingPlugin.Sign.Create`
- `VotingPlugin.TopVoter.Ignore`
- `votingpluginproxy.admin`

## Events

- AuthActionEvent -> loginSecurityLogin (com/bencodez/votingplugin/advancedcore/listeners/LoginSecurityLogin.java) **no SDK event of this name**
- AuthMeAsyncPreLoginEvent -> authmeLogin (com/bencodez/votingplugin/advancedcore/listeners/AuthMeLogin.java) **no SDK event of this name**
- AuthenticateEvent -> nLoginLogin (com/bencodez/votingplugin/advancedcore/listeners/NLoginAuthenticate.java) **no SDK event of this name**
- DateChangedEvent -> onPostDateChange (com/bencodez/votingplugin/advancedcore/api/backup/BackupHandle.java) **no SDK event of this name**
- DayChangeEvent -> onDayChange (com/bencodez/votingplugin/specialrewards/voteparty/VoteParty.java) **no SDK event of this name**
- InventoryClickEvent -> onInventoryClick (com/bencodez/votingplugin/simpleapi/valuerequest/InventoryRequestManager.java)
- InventoryCloseEvent -> onInventoryClose (com/bencodez/votingplugin/simpleapi/valuerequest/InventoryRequestManager.java)
- MonthChangeEvent -> onMonthChange (com/bencodez/votingplugin/specialrewards/voteparty/VoteParty.java) **no SDK event of this name**
- PlayerCommandSendEvent -> onPlayerCommandSend (com/bencodez/votingplugin/commands/tabcompleter/AliasCommandFilterListener.java)
- PluginDisableEvent -> onPluginDisable (com/bencodez/votingplugin/advancedcore/api/misc/effects/ItemMessage.java)
- PluginMessageEvent -> onPluginMessage (com/bencodez/votingplugin/proxy/bungee/VotingPluginBungee.java) **no SDK event of this name**
- SignChangeEvent -> onSignChange (com/bencodez/votingplugin/simpleapi/valuerequest/SignInputManager.java)
- VoteMilestoneRewardEvent -> onVoteMilestoneReward (com/bencodez/votingplugin/votelog/listeners/VoteMilestoneVoteLogListener.java) **no SDK event of this name**
- VotifierEvent -> onVote (com/bencodez/votingplugin/proxy/bungee/VoteEventBungee.java) **no SDK event of this name**
- WeekChangeEvent -> onWeekChange (com/bencodez/votingplugin/specialrewards/voteparty/VoteParty.java) **no SDK event of this name**

## Timers


## Files

- data/votingpluginversion.yml
- data/Shop.yml
- data/Config.yml
- data/VoteSites.yml
- data/GUI.yml
- data/SpecialRewards.yml
- data/BungeeSettings.yml
- data/bungeeconfig.yml
- data/advancedcoreversion.yml
- data/Rewards/ExampleAdvanced.yml
- data/Rewards/ExampleBasic.yml
- data/bundle.properties

## Keys read from config / messages

- `0.0.0.0`
- `ActionBar.Message`
- `Algo`
- `BedrockPlayerPrefix`
- `BroadcastMsg`
- `BungeeMethod`
- `BungeeServer.Host`
- `CHEST.VoteGUI.`
- `CHEST.VoteNext.CustomSiteNamesDisplays.`
- `CHEST.VoteShopDisabled`
- `CHEST.VoteShopLimitReached`
- `CHEST.VoteShopName`
- `COLUMN_DEFAULT`
- `Category`
- `ChannelID`
- `CheckWorld`
- `Color`
- `Command`
- `ConditionalJavascript`
- `ContentType`
- `CustomPlaceholderReturns.`
- `DATA_TYPE`
- `Database`
- `DayVoteStreakLastUpdate`
- `DbType`
- `Default`
- `Driver`
- `Duration`
- `Effect`
- `Expression`
- `FallBackServer`
- `Format.AlternateBroadcast.Broadcast`
- `Format.BroadcastMsg`
- `Group`
- `Header`
- `HelpMessage`
- `Host`
- `Identifier_Name`
- `InputMethod`
- `Item.Material`
- `ItemModel`
- `Items.`
- `ItemsAdder`
- `JavascriptExpression`
- `LastOnline`
- `LastVotes`
- `Line`
- `ListLine`
- `Lore`
- `MQTT.BrokerURL`
- `MQTT.ClientID`
- `MQTT.Password`
- `MQTT.Prefix`
- `MQTT.Username`
- `Material`
- `Message`
- `Method`
- `Month`
- `MultiProxyMethod`
- `MultiProxyRedis.Host`
- `MultiProxyRedis.Password`
- `MultiProxyRedis.Username`
- `MultiProxySocketHost.Host`
- `NOTSET`
- `Name`
- `Nexo`
- `NonVotedPlayers.`
- `Password`
- `Permission`
- `PermissionToView`
- `PlayerName`
- `PluginMessageChannel`
- `PluginVersions.`
- `PoolName`
- `Prefix`
- `ProxyBroadcast.OfflineMode`
- `ProxyBroadcast.Scope.Mode`
- `ProxyServerName`
- `PurchaseMessage`
- `Redis.Host`
- `Redis.Password`
- `Redis.Prefix`
- `Redis.Username`
- `RewardType`
- `Secret`
- `Server`
- `Service`
- `ServiceSite`
- `Shop.`
- `Signs.`
- `Skull`
- `SkullTexture`
- `SkullURL`
- `SkullUUID`
- `Sound`
- `Style`
- `SubTitle`
- `Time.Month`
- `TimeZone`
- `Title`
- `Total`
- `TotalToUse`
- `Type`
- `Url`
- `Username`
- `VoteDelay`
- `VoteParty.Broadcast`
- `VoteReminderOptions.Defaults.Conditions.MinOnlineTime`
- `VoteReminderOptions.Defaults.Cooldown`
- `VoteReminderOptions.Defaults.Delay`
- `VoteReminderOptions.GlobalCooldown`
- `VoteRemindersMap`
- `VoteURL`
- `WaitUntilVoteDelay.`
- `WeekDay`
- `World`
- `buildnumber`
- `context`
- `data_type`
- `destination`
- `event`
- `payload`
- `playerName`
- `player_name`
- `player_uuid`
- `profile`
- `server`
- `serverUuid`
- `service`
- `source`
- `status`
- `text`
- `time`
- `uuid`
- `version`
- `voteId`
- `vote_id`
- `voteid`

## Biggest classes (write these first, they are most of the work)

- 188 KB com/bencodez/votingplugin/commands/CommandLoader.java
- 138 KB com/bencodez/votingplugin/advancedcore/api/rewards/RewardHandler.java
- 77 KB com/bencodez/votingplugin/VotingPluginMain.java
- 75 KB com/bencodez/votingplugin/proxy/VotingPluginProxy.java
- 59 KB com/bencodez/votingplugin/simpleapi/hikari/pool/HikariProxyCallableStatement.java
- 55 KB com/bencodez/votingplugin/specialrewards/votemilestones/VoteMilestonesManager.java
- 54 KB com/bencodez/votingplugin/specialrewards/votestreak/VoteStreakHandler.java
- 51 KB com/bencodez/votingplugin/advancedcore/api/item/ItemBuilder.java
- 51 KB com/bencodez/votingplugin/placeholders/PlaceHolders.java
- 46 KB com/bencodez/votingplugin/config/Config.java
- 44 KB com/bencodez/votingplugin/simpleapi/hikari/pool/HikariProxyResultSet.java
- 43 KB com/bencodez/votingplugin/user/VotingPluginUser.java
- 42 KB com/bencodez/votingplugin/simpleapi/hikari/pool/HikariProxyDatabaseMetaData.java
- 39 KB com/bencodez/votingplugin/advancedcore/AdvancedCorePlugin.java
- 37 KB com/bencodez/votingplugin/topvoter/TopVoterHandler.java
- 37 KB com/bencodez/votingplugin/votelog/VoteLogMysqlTable.java
- 34 KB com/bencodez/votingplugin/advancedcore/api/user/AdvancedCoreUser.java
- 32 KB com/bencodez/votingplugin/votereminding/VoteRemindersManager.java
- 32 KB com/bencodez/votingplugin/advancedcore/command/CommandLoader.java
- 31 KB com/bencodez/votingplugin/BungeeHandler.java
- 31 KB com/bencodez/votingplugin/proxy/velocity/VotingPluginVelocity.java
- 30 KB com/bencodez/votingplugin/advancedcore/api/user/userstorage/mysql/MySQL.java
- 29 KB com/bencodez/votingplugin/simpleapi/valuerequest/ValueRequest.java
- 28 KB com/bencodez/votingplugin/simpleapi/hikari/pool/HikariPool.java
- 28 KB com/bencodez/votingplugin/simpleapi/hikari/pool/HikariProxyPreparedStatement.java

## Kit map

- messages: `msg("key", &[args])` (Lang: {prefix}, %s, {0}); settings: `config(|c| c.int("path", default))`
- commands: the Router above; a sub = `fn(&Ctx) -> Res` (`c.player()?`, `c.int(0, "amount")?`, `c.online(0)?`, `c.tell(..)`)
- client-only entities (holograms, markers, NPCs): `horizon_kit::packets` (+ `wire` for raw bytes)
- per-tick work: `horizon_kit::worker` (server-thread cost ~0, as Java async)
- a `/x profiler`: `horizon_kit::stats::{set_running, report, timed}`
- any Bukkit call: `mcp plugin-port api_lookup <Class.method>` / `sdk_search`
