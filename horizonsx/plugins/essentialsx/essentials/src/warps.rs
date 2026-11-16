//! `Warps`: one file per warp in `warps/`, the location at the root with `name` and `lastowner`.
use crate::prelude::*;
use crate::config::Config;
use crate::config_entities::LazyLocation;
use crate::utils::string::{sanitize_file_name, to_uuid};
use serde_yaml::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct Warps {
    /// By lower case name, keeping the name as written.
    points: Mutex<BTreeMap<String, (String, Config)>>,
    folder: PathBuf,
}

impl Warps {
    pub fn new() -> Self {
        let folder = crate::data_folder().join("warps");
        let _ = std::fs::create_dir_all(&folder);
        let warps = Self { points: Mutex::new(BTreeMap::new()), folder };
        warps.reload_config();
        warps
    }
    fn points(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, (String, Config)>> {
        self.points.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn is_empty(&self) -> bool {
        self.points().is_empty()
    }
    /// `isWarp`.
    pub fn is_warp(&self, name: &str) -> bool {
        self.points().contains_key(&name.to_lowercase())
    }
    /// `getList`: sorted, ignoring case.
    pub fn get_list(&self) -> Vec<String> {
        self.points().values().map(|(name, _)| name.clone()).collect()
    }
    /// `getWarp`.
    pub fn get_warp(&self, warp: &str) -> EssResult<Location> {
        let points = self.points();
        let (_, config) = points.get(&warp.to_lowercase()).ok_or_else(EssError::warp_not_found)?;
        config.get_location(None).and_then(|mut place| place.location()).ok_or_else(EssError::warp_not_found)
    }
    /// `setWarp`.
    pub fn set_warp(&self, user: Option<&User>, name: &str, location: &Location) -> EssResult {
        let mut points = self.points();
        let key = name.to_lowercase();
        if !points.contains_key(&key) {
            let file = self.folder.join(format!("{}.yml", sanitize_file_name(name)));
            fail_if!(file.exists(), EssError::tl("similarWarpExist", &[]));
            let mut config = Config::new(file);
            config.load();
            points.insert(key.clone(), (name.to_owned(), config));
        }
        let Some((_, config)) = points.get_mut(&key) else { return Ok(()) };
        config.set_location(None, &LazyLocation::from_location(location));
        config.set("name", Value::from(name));
        if let Some(user) = user {
            config.set("lastowner", Value::from(user.get_uuid()));
        }
        config.save();
        Ok(())
    }
    /// `getLastOwner`.
    pub fn get_last_owner(&self, warp: &str) -> EssResult<Option<String>> {
        let points = self.points();
        let (_, config) = points.get(&warp.to_lowercase()).ok_or_else(EssError::warp_not_found)?;
        Ok(config.get_string("lastowner", None).and_then(|owner| to_uuid(&owner)))
    }
    /// `removeWarp`.
    pub fn remove_warp(&self, name: &str) -> EssResult {
        let key = name.to_lowercase();
        let mut points = self.points();
        let (_, config) = points.get(&key).ok_or_else(|| EssError::tl("warpNotExist", &[]))?;
        crate::config::flush();
        fail_if!(std::fs::remove_file(config.get_file()).is_err(), EssError::tl("warpDeleteError", &[]));
        points.remove(&key);
        Ok(())
    }
    /// `reloadConfig`.
    pub fn reload_config(&self) {
        let mut points = self.points();
        points.clear();
        let Ok(files) = std::fs::read_dir(&self.folder) else { return };
        for file in files.flatten() {
            let path = file.path();
            if !path.is_file() || path.extension().and_then(|extension| extension.to_str()) != Some("yml") {
                continue;
            }
            let mut config = Config::new(path.clone());
            config.load();
            match config.get_string("name", None) {
                Some(name) if config.has_property("world") => {
                    points.insert(name.to_lowercase(), (name, config));
                }
                Some(_) | None => {
                    if !config.has_property("world") && config.get_keys().is_empty() {
                        crate::warning(&crate::adventure::mini_to_legacy(&tl_literal("loadWarpError", crate::args![path.display().to_string()])));
                    }
                }
            }
        }
    }
    /// `getCount`.
    pub fn get_count(&self) -> usize {
        self.points().len()
    }
}
