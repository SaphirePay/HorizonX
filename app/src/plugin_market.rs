//! Plugins from Modrinth for a server: search the Paper, Spigot, Bukkit, Purpur and Folia plugins made for the
//! servers' game version, and install one (its newest build for that version, with the plugins it needs) into a
//! server's `plugins/` folder, where the Java layer runs it. What came from Modrinth is remembered in
//! `plugins/.modrinth.json`, so the list can say "Installed" and an update replaces the old jar.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const API: &str = "https://api.modrinth.com/v2";
/// The game version every HorizonX server plays.
pub const GAME_VERSION: &str = "26.3";
/// The plugin platforms the Java layer runs.
const LOADERS: [&str; 5] = ["paper", "spigot", "bukkit", "purpur", "folia"];
const PAGE: usize = 20;

/// One plugin in the results.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hit {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub author: String,
    pub description: String,
    pub downloads: u64,
}

impl Hit {
    /// Its page on Modrinth.
    pub fn url(&self) -> String {
        format!("https://modrinth.com/plugin/{}", self.slug)
    }
}

/// A page of results and how many there are in all.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Results {
    pub hits: Vec<Hit>,
    pub total: usize,
}

/// What an install did.
#[derive(Clone, Debug, Default)]
pub struct Installed {
    /// The plugin's name and the jar it put in.
    pub name: String,
    pub file: String,
    /// Plugins it needed and also put in (by name).
    pub with: Vec<String>,
    /// It was there already, at this version.
    pub already: bool,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(60))
        .user_agent(concat!("HorizonX/", env!("CARGO_PKG_VERSION"), " (plugin browser)"))
        .build()
}

fn get(url: &str) -> Result<Value, String> {
    agent()
        .get(url)
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(429, _) => "Modrinth asks to slow down: try again in a minute.".into(),
            ureq::Error::Status(code, _) => format!("Modrinth answered {code}."),
            ureq::Error::Transport(t) => format!("No connection to Modrinth: {t}"),
        })?
        .into_json()
        .map_err(|e| format!("Modrinth's answer wasn't readable: {e}"))
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_owned()
}

/// Plugins for the servers' game version, best match first (most downloaded when `query` is empty); `offset`
/// pages through them.
pub fn search(query: &str, offset: usize) -> Result<Results, String> {
    let loaders: Vec<String> = LOADERS.iter().map(|l| format!("\"categories:{l}\"")).collect();
    let facets = format!("[[\"project_type:plugin\"],[\"versions:{GAME_VERSION}\"],[{}]]", loaders.join(","));
    let index = if query.trim().is_empty() { "downloads" } else { "relevance" };
    let url = format!(
        "{API}/search?query={}&facets={}&index={index}&limit={PAGE}&offset={offset}",
        encode(query.trim()),
        encode(&facets)
    );
    let v = get(&url)?;
    let hits = v["hits"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|h| Hit {
                    id: text(&h["project_id"]),
                    slug: text(&h["slug"]),
                    title: text(&h["title"]),
                    author: text(&h["author"]),
                    description: text(&h["description"]),
                    downloads: h["downloads"].as_u64().unwrap_or(0),
                })
                .filter(|h| !h.id.is_empty())
                .collect()
        })
        .unwrap_or_default();
    Ok(Results { hits, total: v["total_hits"].as_u64().unwrap_or(0) as usize })
}

/// The Modrinth plugins in a server: project id -> what we put in.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Record {
    pub name: String,
    pub file: String,
    pub version: String,
}

fn records_path(dir: &Path) -> PathBuf {
    dir.join("plugins").join(".modrinth.json")
}

/// What came from Modrinth and is still there.
pub fn installed(dir: &Path) -> BTreeMap<String, Record> {
    let all: BTreeMap<String, Record> = fs::read_to_string(records_path(dir)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let plugins = dir.join("plugins");
    all.into_iter()
        .filter(|(_, r)| plugins.join(&r.file).is_file() || plugins.join(format!("{}.disabled", r.file)).is_file())
        .collect()
}

fn save_records(dir: &Path, all: &BTreeMap<String, Record>) -> Result<(), String> {
    let text = serde_json::to_string_pretty(all).map_err(|e| e.to_string())?;
    fs::write(records_path(dir), text).map_err(|e| e.to_string())
}

/// The newest build of a plugin for the game version and the Java layer's platforms: (version id, version
/// number, file name, download URL, required plugins' project ids).
fn newest(project: &str) -> Result<(String, String, String, String, Vec<String>), String> {
    let loaders = format!("[{}]", LOADERS.iter().map(|l| format!("\"{l}\"")).collect::<Vec<_>>().join(","));
    let games = format!("[\"{GAME_VERSION}\"]");
    let url = format!("{API}/project/{}/version?loaders={}&game_versions={}", encode(project), encode(&loaders), encode(&games));
    let versions = get(&url)?;
    let list = versions.as_array().cloned().unwrap_or_default();
    // A release when there is one, else the newest beta or alpha.
    let pick = list
        .iter()
        .find(|v| v["version_type"] == "release")
        .or_else(|| list.first())
        .ok_or(format!("It has no build for Minecraft {GAME_VERSION} yet."))?;
    let files = pick["files"].as_array().cloned().unwrap_or_default();
    let file = files
        .iter()
        .find(|f| f["primary"].as_bool() == Some(true) && text(&f["filename"]).ends_with(".jar"))
        .or_else(|| files.iter().find(|f| text(&f["filename"]).ends_with(".jar")))
        .ok_or("Its build has no plugin jar.")?;
    let needs = pick["dependencies"]
        .as_array()
        .map(|d| {
            d.iter()
                .filter(|d| d["dependency_type"] == "required")
                .filter_map(|d| d["project_id"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Ok((text(&pick["id"]), text(&pick["version_number"]), text(&file["filename"]), text(&file["url"]), needs))
}

fn project_name(project: &str) -> String {
    get(&format!("{API}/project/{}", encode(project))).map(|p| text(&p["title"])).unwrap_or_else(|_| project.to_owned())
}

/// Puts a plugin (by Modrinth project id or slug) into a server, with the plugins it needs. Replaces an older
/// jar of the same plugin. The server loads it at its next start.
pub fn install(dir: &Path, project: &str) -> Result<Installed, String> {
    let folder = dir.join("plugins");
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let mut records = installed(dir);
    let mut out = Installed::default();
    let mut todo = vec![(project.to_owned(), 0usize)];
    let mut seen = Vec::new();
    while let Some((id, depth)) = todo.pop() {
        if seen.contains(&id) || depth > 3 {
            continue;
        }
        seen.push(id.clone());
        let (_, version, file, url, needs) = newest(&id)?;
        let name = project_name(&id);
        let fresh = records.get(&id).is_none_or(|r| r.file != file);
        if fresh {
            let tmp = folder.join(format!("{file}.part"));
            let mut reader = agent().get(&url).call().map_err(|e| format!("Downloading {name}: {e}"))?.into_reader();
            let mut w = fs::File::create(&tmp).map_err(|e| e.to_string())?;
            std::io::copy(&mut reader, &mut w).map_err(|e| format!("Downloading {name}: {e}"))?;
            drop(w);
            fs::rename(&tmp, folder.join(&file)).map_err(|e| e.to_string())?;
            // The old jar of this plugin goes when the new one is in.
            if let Some(old) = records.get(&id).filter(|r| r.file != file) {
                let _ = fs::remove_file(folder.join(&old.file));
                let _ = fs::remove_file(folder.join(format!("{}.disabled", old.file)));
            }
            records.insert(id.clone(), Record { name: name.clone(), file: file.clone(), version });
        }
        if depth == 0 {
            out.name = name;
            out.file = file;
            out.already = !fresh;
        } else if fresh {
            out.with.push(name);
        }
        for need in needs {
            // Something the server already has from Modrinth stays.
            if !records.contains_key(&need) {
                todo.push((need, depth + 1));
            }
        }
    }
    save_records(dir, &records)?;
    crate::host::ensure_java_layer(dir)?;
    Ok(out)
}

/// "1.2M", "45.3k", "812".
pub fn short_count(n: u64) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{:.1}k", n as f64 / 1e3),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real install from Modrinth into an empty server folder (network): `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn installs_a_plugin_from_modrinth() {
        let dir = std::env::temp_dir().join(format!("hh-market-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let found = search("chunky", 0).unwrap();
        let hit = found.hits.iter().find(|h| h.slug == "chunky").expect("Chunky in the results");
        let done = install(&dir, &hit.id).unwrap();
        assert!(dir.join("plugins").join(&done.file).is_file(), "{done:?}");
        assert!(installed(&dir).contains_key(&hit.id));
        let again = install(&dir, &hit.id).unwrap();
        assert!(again.already);
        let _ = fs::remove_dir_all(&dir);
    }
}
