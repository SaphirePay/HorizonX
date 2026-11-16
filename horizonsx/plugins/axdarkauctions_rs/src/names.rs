//! Item names in the configured `language` (LanguageManager): the game's lang file for that language, fetched once
//! from the minecraft-assets mirror and kept in `libs/translations/<language>.yml` (`material.<id>: name`). Until
//! it is there, or when it can't be fetched, an item reads as its id in words (`iron ingot`), as in Java.

use horizon_kit::yaml;
use serde_yaml::Value as Yaml;
use std::collections::HashMap;
use std::sync::Mutex;

static NAMES: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn file(folder: &std::path::Path, language: &str) -> std::path::PathBuf {
    folder.join("libs").join("translations").join(format!("{language}.yml"))
}

fn read(path: &std::path::Path) -> HashMap<String, String> {
    let doc = yaml::read(path);
    yaml::keys(&doc, "material").into_iter().filter_map(|k| yaml::string(&doc, &format!("material.{k}")).map(|v| (k, v))).collect()
}

/// Loads the kept file; fetches it on a thread when it isn't there yet.
pub fn reload(folder: std::path::PathBuf, language: &str) {
    let language = language.to_ascii_lowercase();
    let path = file(&folder, &language);
    let kept = read(&path);
    let have = !kept.is_empty();
    *NAMES.lock().unwrap_or_else(|e| e.into_inner()) = Some(kept);
    if have {
        return;
    }
    let _ = std::thread::Builder::new().name("AxDarkAuctions-lang".into()).spawn(move || {
        crate::texts::console(&format!("&#00DDFF╠ &#00FFFFDownloading &f{language} &#00FFFFlanguage files.."));
        let Some(names) = fetch(&language) else {
            crate::texts::console("&#FF3333[AxDarkAuctions] Could not download the item names; items show their ids.");
            return;
        };
        let mut doc = serde_yaml::Mapping::new();
        let materials: serde_yaml::Mapping = names.iter().map(|(k, v)| (Yaml::String(k.clone()), Yaml::String(v.clone()))).collect();
        doc.insert(Yaml::String("material".into()), Yaml::Mapping(materials));
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_yaml::to_string(&Yaml::Mapping(doc)) {
            let _ = std::fs::write(&path, text);
        }
        *NAMES.lock().unwrap_or_else(|e| e.into_inner()) = Some(names);
    });
}

fn fetch(language: &str) -> Option<HashMap<String, String>> {
    for version in ["1.21.10", "1.21.8", "1.21.5", "1.21.4"] {
        let url = format!("https://raw.githubusercontent.com/InventivetalentDev/minecraft-assets/{version}/assets/minecraft/lang/{language}.json");
        let Ok(response) = ureq::get(&url).timeout(std::time::Duration::from_secs(15)).call() else { continue };
        let Ok(body) = response.into_string() else { continue };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) else { continue };
        let mut out = HashMap::new();
        for (key, value) in json.as_object()? {
            let id = key.strip_prefix("item.minecraft.").or_else(|| key.strip_prefix("block.minecraft."));
            if let (Some(id), Some(text)) = (id, value.as_str())
                && !id.contains('.')
            {
                out.insert(id.to_owned(), text.to_owned());
            }
        }
        if !out.is_empty() {
            return Some(out);
        }
    }
    None
}

/// `material` as `minecraft:iron_ingot` or `IRON_INGOT`: its name in the language.
pub fn translated(material: &str) -> String {
    let id = material.trim_start_matches("minecraft:").to_ascii_lowercase();
    NAMES.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&id).cloned()).unwrap_or_else(|| id.replace('_', " "))
}
