//! `plugin.yml` for the bundle: Essentials' own, with each module's commands and permissions added
//! for the features this build has.
use serde_yaml::Value;
use std::env;
use std::fs;
use std::path::Path;

const MODULES: [(&str, &str); 8] = [("DISCORD", "essentials-discord"), ("DISCORDLINK", "essentials-discordlink"), ("XMPP", "essentials-xmpp"), ("CHAT", "essentials-chat"), ("SPAWN", "essentials-spawn"), ("ANTIBUILD", "essentials-antibuild"), ("PROTECT", "essentials-protect"), ("GEOIP", "essentials-geoip")];

fn merge(into: &mut Value, from: &Value, key: &str) {
    if let (Some(into), Some(from)) = (into.get_mut(key).and_then(Value::as_mapping_mut), from.get(key).and_then(Value::as_mapping)) {
        for (name, value) in from {
            into.insert(name.clone(), value.clone());
        }
    }
}

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("cargo sets the manifest folder");
    let root = Path::new(&manifest);
    let parent = root.parent().expect("plugin-ports");
    let core = parent.join("essentials/plugin.yml");
    println!("cargo:rerun-if-changed={}", core.display());
    let text = fs::read_to_string(&core).expect("essentials/plugin.yml");
    let mut plugin: Value = serde_yaml::from_str(&text.replace("${full.version}", "2.22.0")).expect("plugin.yml");
    for (feature, folder) in MODULES {
        if env::var(format!("CARGO_FEATURE_{feature}")).is_err() {
            continue;
        }
        let file = parent.join(folder).join("plugin.yml");
        println!("cargo:rerun-if-changed={}", file.display());
        let module: Value = serde_yaml::from_str(&fs::read_to_string(&file).expect("a module's plugin.yml").replace("${full.version}", "2.22.0")).expect("a module's plugin.yml");
        merge(&mut plugin, &module, "commands");
        merge(&mut plugin, &module, "permissions");
    }
    let generated = serde_yaml::to_string(&plugin).expect("plugin.yml");
    let target = root.join("plugin.yml");
    // The file is shared by every feature set, and each keeps its own cached run of this script: a
    // build with other features would otherwise ship what the last one wrote.
    println!("cargo:rerun-if-changed={}", target.display());
    if fs::read_to_string(&target).ok().as_deref() != Some(generated.as_str()) {
        fs::write(&target, generated).expect("plugin.yml");
    }
}
