//! config.conf and lang.conf (HOCON, field names as PlayerVaultsX writes them), with the Java defaults; an old
//! config.yml is imported once as `Config.setFromConfig` does.

use crate::hocon;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct Settings {
    pub debug: bool,
    pub rows: usize,
    pub signs: bool,
    pub eco: bool,
    pub fee_create: f64,
    pub fee_open: f64,
    pub refund: f64,
    pub blocking: bool,
    pub blocked: Vec<String>,
    pub models: Vec<String>,
    pub models_whitelist: bool,
    pub enchants: Vec<String>,
    pub purge: Option<i64>,
    pub max_test: u32,
    pub backups: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            debug: false,
            rows: 6,
            signs: false,
            eco: false,
            fee_create: 100.0,
            fee_open: 10.0,
            refund: 50.0,
            blocking: true,
            blocked: vec!["PUMPKIN".into()],
            models: Vec::new(),
            models_whitelist: false,
            enchants: Vec::new(),
            purge: None,
            max_test: 99,
            backups: true,
        }
    }
}

const DEFAULT_CONFIG: &str = r#"# PlayerVaults
# Created by: https://github.com/drtshock/PlayerVaults/graphs/contributors/
# Resource page: https://www.spigotmc.org/resources/51204/
# Discord server: https://discordapp.com/invite/JZcWDEt/
# Made with love <3
aPleasantHello=true
# Debug Mode
#  This will print everything the plugin is doing to console.
#  You should only enable this if you're working with a contributor to fix something.
debug=false
# Can be 1 through 6.
# Default: 6
defaultVaultRows=6
# Economy
#  These are all of the settings for the economy integration. (Requires Vault)
#   Bypass permission is: playervaults.free
economy {
    # Set me to true to enable economy features!
    enabled=false
    feeToCreate=100
    feeToOpen=10
    refundOnDelete=50
}
# Blocked Items
#  This will allow you to block specific materials from vaults.
#   Bypass permission is: playervaults.bypassblockeditems
itemBlocking {
    enabled=true
    # Enchantments to block from entering a vault at all.
    enchantmentsBlocked=[]
    # Material list for blocked items (does not support ID's), only effective if the feature is enabled.
    #  If you don't know material names: https://hub.spigotmc.org/javadocs/bukkit/org/bukkit/Material.html
    #
    # Also, if you add "BLOCK_ALL_WITH_CUSTOM_MODEL_DATA" or "BLOCK_ALL_WITHOUT_CUSTOM_MODEL_DATA"
    #  then either all items with custom model data will be blocked, or all items without custom model data will be blocked.
    #
    # Also, if you add "BLOCK_ALL_WITH_CUSTOM_MODEL" or "BLOCK_ALL_WITHOUT_CUSTOM_MODEL"
    #  then either all items with a custom model will be blocked, or all items without a custom model will be blocked.
    list=[
        PUMPKIN
    ]
    # List models (* for wildcard) you wish to block. Toggle modelListWhitelistInstead to make it a whitelist, blocking anything not on it.
    modelList=[]
    modelListWhitelistInstead=false
}
# Sets the highest vault amount this plugin will test perms for
maxVaultAmountPermTest=99
# Cleanup
#  Enabling this will purge vaults that haven't been touched in the specified time frame.
#   Reminder: This is only checked during startup.
#             This will not lag your server or touch the backups folder.
purge {
    # Time, in days, since last edit
    daysSinceLastEdit=30
    enabled=false
}
# Signs
#  This will determine whether vault signs are enabled.
#  If you don't know what this is or if it's for you, see the resource page.
signs=false
# Storage option. Currently only flatfile, but soon more! :)
storage {
    flatFile {
        # Backups
        #  Enabling this will create backups of vaults automagically.
        backups=true
    }
    storageType=flatfile
}
"#;

pub const TRANSLATIONS: [(&str, &str); 32] = [
    ("openVault", "<normal>Opening vault <info><vault></info>"),
    ("openOtherVault", "<normal>Opening vault <info><vault></info> of <info><player></info>"),
    ("invalidArgs", "<error>Invalid args!"),
    ("deleteVault", "<normal>Deleted vault <info><vault></info>"),
    ("deleteOtherVault", "<normal>Deleted vault <info><vault></info> <normal>of <info><player></info>"),
    ("deleteOtherVaultAll", "<dark_red>Deleted all vaults belonging to <info><player></info>"),
    ("playerOnly", "<error>Sorry but that can only be run by a player!"),
    ("mustBeNumber", "<error>You need to specify a valid number."),
    ("noPerms", "<error>You don't have permission for that!"),
    ("insufficientFunds", "<error>You don't have enough money for that!"),
    ("refundAmount", "<normal>You were refunded <info><price></info> for deleting that vault."),
    ("costToCreate", "<normal>You were charged <info><price></info> for creating a vault."),
    ("costToOpen", "<normal>You were charged <info><price></info> for opening that vault."),
    ("vaultDoesNotExist", "<error>That vault does not exist!"),
    ("clickASign", "<normal>Now click a sign!"),
    ("notASign", "<error>You must click a sign!"),
    ("setSign", "<normal>You have successfully set a PlayerVault access sign!"),
    ("existingVaults", "<normal><player> has vaults: <info><vault></info>"),
    ("vaultTitle", "<dark_red>Vault #<vault>"),
    ("openWithSign", "<normal>Opening vault <info><vault></info> of <info><player></info>"),
    ("noOwnerFound", "<error>Cannot find vault owner: <info><player></info>"),
    ("convertPluginNotFound", "<error>No converter found for that plugin."),
    ("convertComplete", "<normal>Converted <info><count></info> players to PlayerVaults."),
    ("convertBackground", "<normal>Conversion has been forked to the background. See console for updates."),
    ("locked", "<error>Vaults are currently locked while conversion occurs. Please try again in a moment!"),
    ("help", "/pv <number>"),
    ("blockedItem", "<gold><item></gold> <error>is blocked from vaults."),
    ("blockedItemWithModelData", "<error>This item is blocked from vaults."),
    ("blockedItemWithoutModelData", "<error>This item is blocked from vaults."),
    ("blockedItemModel", "<error>This item is blocked from vaults."),
    ("blockedItemWithEnchantments", "<error>This item's enchantments are blocked from vaults."),
    ("signsDisabled", "<error>Vault signs are currently disabled."),
];

const DEFAULT_TITLE: &str = "<dark_red>[<normal>PlayerVaults<dark_red>]: ";

fn default_lang() -> String {
    let mut s = String::new();
    s.push_str("# https://docs.adventure.kyori.net/minimessage.html#format\ncolorMappings {\n    error=red\n    info=green\n    normal=white\n}\nplaceholders {\n");
    s.push_str(&format!("    title={}\n}}\ntranslations {{\n", hocon::quote(DEFAULT_TITLE)));
    for (k, v) in TRANSLATIONS {
        s.push_str(&format!("    {k}={}\n", hocon::quote(v)));
    }
    s.push_str("}\n");
    s
}

#[derive(Clone, Debug, Default)]
pub struct Lang {
    pub title: String,
    pub colors: Vec<(String, String)>,
    pub lines: HashMap<String, Vec<String>>,
}

fn lines_of(v: &Value) -> Option<Vec<String>> {
    match v {
        Value::String(s) => Some(vec![s.clone()]),
        Value::Array(a) => Some(a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()),
        _ => None,
    }
}

impl Lang {
    pub fn get(&self, key: &str) -> Vec<String> {
        self.lines.get(key).cloned().or_else(|| TRANSLATIONS.iter().find(|(k, _)| *k == key).map(|(_, v)| vec![(*v).to_owned()])).unwrap_or_default()
    }

    /// One line as MiniMessage ready to parse: the colour names mapped, `<key>` values put in (as plain text).
    pub fn prepare(&self, line: &str, pairs: &[(&str, String)], title: bool) -> String {
        let mut l = if title { format!("{}{line}", self.title) } else { line.to_owned() };
        for (k, v) in pairs {
            // Values are text, not tags: their `<` escaped.
            l = l.replace(&format!("<{k}>"), &v.replace('<', "\\<"));
        }
        for (name, color) in &self.colors {
            let c = color.trim();
            l = l.replace(&format!("<{name}>"), &format!("<{c}>")).replace(&format!("</{name}>"), &format!("</{c}>"));
        }
        l
    }
}

fn num(v: &Value, path: &str) -> Option<f64> {
    pointer(v, path).and_then(|x| x.as_f64().or_else(|| x.as_str().and_then(|s| s.parse().ok())))
}

fn boolean(v: &Value, path: &str) -> Option<bool> {
    pointer(v, path).and_then(|x| x.as_bool().or_else(|| x.as_str().map(|s| s.eq_ignore_ascii_case("true"))))
}

fn list(v: &Value, path: &str) -> Option<Vec<String>> {
    pointer(v, path).and_then(|x| match x {
        Value::Array(a) => Some(a.iter().filter_map(|i| i.as_str().map(str::to_owned).or_else(|| (!i.is_null()).then(|| i.to_string()))).collect()),
        Value::String(s) => Some(vec![s.clone()]),
        _ => None,
    })
}

fn pointer<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = v;
    for k in path.split('.') {
        cur = cur.get(k)?;
    }
    Some(cur)
}

/// The old config.yml as config.conf (`setFromConfig`), the yml kept as old_unused_config.yml.
fn import_yml(folder: &Path) -> Option<String> {
    let yml = folder.join("config.yml");
    let doc: serde_yaml::Value = horizon_kit::yaml::parse(&std::fs::read_to_string(&yml).ok()?).ok()?;
    let g = |k: &str| {
        let mut cur = &doc;
        for part in k.split('.') {
            cur = cur.get(part)?;
        }
        Some(cur.clone())
    };
    let b = |k: &str, d: bool| g(k).and_then(|v| v.as_bool()).unwrap_or(d);
    let n = |k: &str, d: f64| g(k).and_then(|v| v.as_f64()).unwrap_or(d);
    let blocked: Vec<String> = g("blocked-items").and_then(|v| v.as_sequence().map(|s| s.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())).unwrap_or_else(|| vec!["PUMPKIN".into(), "DIAMOND_BLOCK".into()]);
    let mut text = DEFAULT_CONFIG.to_owned();
    let set = |text: &mut String, from: &str, to: String| *text = text.replacen(from, &to, 1);
    set(&mut text, "debug=false", format!("debug={}", b("debug", false)));
    set(&mut text, "signs=false", format!("signs={}", b("signs-enabled", false)));
    set(&mut text, "    enabled=false\n    feeToCreate=100", format!("    enabled={}\n    feeToCreate={}", b("economy.enabled", false), n("economy.cost-to-create", 100.0)));
    set(&mut text, "feeToOpen=10", format!("feeToOpen={}", n("economy.cost-to-open", 10.0)));
    set(&mut text, "refundOnDelete=50", format!("refundOnDelete={}", n("economy.refund-on-delete", 50.0)));
    set(&mut text, "itemBlocking {\n    enabled=true", format!("itemBlocking {{\n    enabled={}", b("blockitems", true)));
    set(&mut text, "list=[\n        PUMPKIN\n    ]", format!("list=[\n{}\n    ]", blocked.iter().map(|m| format!("        {}", hocon::quote(m))).collect::<Vec<_>>().join(",\n")));
    set(&mut text, "daysSinceLastEdit=30\n    enabled=false", format!("daysSinceLastEdit={}\n    enabled={}", n("cleanup.lastEdit", 30.0) as i64, b("cleanup.enable", false)));
    set(&mut text, "backups=true", format!("backups={}", b("backups.enabled", true)));
    set(&mut text, "maxVaultAmountPermTest=99", format!("maxVaultAmountPermTest={}", n("max-vault-amount-perm-to-test", 99.0) as i64));
    let _ = std::fs::rename(&yml, folder.join("old_unused_config.yml"));
    Some(text)
}

/// Both files read (written with defaults when missing). Disk only: a background thread runs it.
pub fn load(folder: &Path) -> (Settings, Lang) {
    let _ = std::fs::create_dir_all(folder);
    let cpath = folder.join("config.conf");
    if !cpath.exists() {
        let text = import_yml(folder).unwrap_or_else(|| DEFAULT_CONFIG.to_owned());
        let _ = std::fs::write(&cpath, text);
    }
    let lpath = folder.join("lang.conf");
    if !lpath.exists() {
        let _ = std::fs::write(&lpath, default_lang());
    }
    let c = hocon::parse(&std::fs::read_to_string(&cpath).unwrap_or_default());
    let d = Settings::default();
    let rows = num(&c, "defaultVaultRows").map_or(6, |r| r as i64);
    let settings = Settings {
        debug: boolean(&c, "debug").unwrap_or(d.debug),
        rows: if (1..=6).contains(&rows) { rows as usize } else { 6 },
        signs: boolean(&c, "signs").unwrap_or(d.signs),
        eco: boolean(&c, "economy.enabled").unwrap_or(d.eco),
        fee_create: num(&c, "economy.feeToCreate").unwrap_or(d.fee_create),
        fee_open: num(&c, "economy.feeToOpen").unwrap_or(d.fee_open),
        refund: num(&c, "economy.refundOnDelete").unwrap_or(d.refund),
        blocking: boolean(&c, "itemBlocking.enabled").unwrap_or(d.blocking),
        blocked: list(&c, "itemBlocking.list").unwrap_or(d.blocked).into_iter().map(|m| m.to_ascii_uppercase()).collect(),
        models: list(&c, "itemBlocking.modelList").unwrap_or_default(),
        models_whitelist: boolean(&c, "itemBlocking.modelListWhitelistInstead").unwrap_or(false),
        enchants: list(&c, "itemBlocking.enchantmentsBlocked").unwrap_or_default(),
        purge: boolean(&c, "purge.enabled").unwrap_or(false).then(|| num(&c, "purge.daysSinceLastEdit").unwrap_or(30.0) as i64),
        max_test: num(&c, "maxVaultAmountPermTest").map_or(99, |n| n.max(1.0) as u32),
        backups: boolean(&c, "storage.flatFile.backups").unwrap_or(true),
    };
    let l = hocon::parse(&std::fs::read_to_string(&lpath).unwrap_or_default().replace('§', "&"));
    let mut lang = Lang { title: DEFAULT_TITLE.to_owned(), colors: vec![("error".into(), "red".into()), ("info".into(), "green".into()), ("normal".into(), "white".into())], lines: HashMap::new() };
    if let Some(t) = pointer(&l, "placeholders.title").and_then(lines_of).and_then(|t| t.into_iter().next()) {
        lang.title = t;
    }
    if let Some(Value::Object(m)) = pointer(&l, "colorMappings") {
        lang.colors = m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned()))).collect();
    }
    if let Some(Value::Object(m)) = pointer(&l, "translations") {
        for (k, v) in m {
            if let Some(lines) = lines_of(v) {
                lang.lines.insert(k.clone(), lines);
            }
        }
    }
    (settings, lang)
}

#[cfg(test)]
mod tests {
    #[test]
    fn default_lang_reads_back() {
        let v = crate::hocon::parse(&super::default_lang());
        assert_eq!(v["translations"]["openVault"], "<normal>Opening vault <info><vault></info>");
        assert_eq!(v["colorMappings"]["info"], "green");
        let c = crate::hocon::parse(super::DEFAULT_CONFIG);
        assert_eq!(c["itemBlocking"]["list"][0], "PUMPKIN");
        assert_eq!(c["storage"]["flatFile"]["backups"], true);
    }
}
