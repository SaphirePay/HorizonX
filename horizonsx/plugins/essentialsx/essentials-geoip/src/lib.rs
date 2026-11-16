//! `EssentialsGeoIP`, on a `.mmdb` file in its folder. `AsyncUserDataLoadEvent` is the join here, a tick later.
mod mmdb;

use essentials::prelude::*;
use essentials::config::Config;
use essentials::i18n::current_locale;
use horizon_bukkit::event::PlayerJoinEvent;
use horizon_bukkit::Plugin;
use mmdb::{Database, Field};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

const TEMPLATE: &str = include_str!("../config.yml");

struct State {
    config: Config,
    reader: Option<Database>,
    locales: Vec<String>,
}

fn folder() -> PathBuf {
    essentials::data_folder().parent().map_or_else(|| PathBuf::from("plugins/EssentialsGeoIP"), |plugins| plugins.join("EssentialsGeoIP"))
}

fn say(text: &str) -> String {
    mini_to_legacy(&tl_literal(text, &[]))
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
    let mut config = Config::with_template(folder().join("config.yml"), TEMPLATE);
    config.load();
    let (reader, locales) = open_database(&config);
    Mutex::new(State { config, reader, locales })
});

/// The `.mmdb` inside a `.tar`.
fn database_in_tar(mut tar: &[u8]) -> Option<&[u8]> {
    while tar.len() >= 512 && tar[0] != 0 {
        let name = String::from_utf8_lossy(&tar[..100]);
        let name = name.trim_end_matches('\0');
        let size = usize::from_str_radix(String::from_utf8_lossy(&tar[124..136]).trim_matches(['\0', ' ']), 8).ok()?;
        let body = tar.get(512..512 + size)?;
        if tar[156] != b'5' && name.to_lowercase().ends_with(".mmdb") {
            return Some(body);
        }
        tar = tar.get(512 + size.div_ceil(512) * 512..)?;
    }
    None
}

/// `downloadDatabase`.
fn download_database(config: &Config, file: &std::path::Path) {
    let key = if config.get_boolean("database.show-cities", false) { "database.download-url-city" } else { "database.download-url" };
    let url = config.string_or(key, "");
    if url.is_empty() {
        essentials::severe(&say("geoIpUrlEmpty"));
        return;
    }
    let license = config.string_or("database.license-key", "");
    if license.is_empty() {
        essentials::severe(&say("geoIpLicenseMissing"));
        return;
    }
    let url = url.replace("{LICENSEKEY}", &license);
    essentials::info(&say("downloadingGeoIp"));
    let agent = ureq::AgentBuilder::new().timeout_connect(std::time::Duration::from_secs(10)).build();
    let mut body = Vec::new();
    let read = agent.get(&url).call().map_err(|_| ()).and_then(|answer| std::io::Read::read_to_end(&mut answer.into_reader(), &mut body).map_err(|_| ()));
    if read.is_err() {
        essentials::severe(&say("connectionFailed"));
        return;
    }
    if url.contains("gz") {
        let mut plain = Vec::new();
        if std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(body.as_slice()), &mut plain).is_err() {
            essentials::severe(&say("connectionFailed"));
            return;
        }
        body = plain;
        if url.contains("tar.gz") {
            let Some(database) = database_in_tar(&body) else {
                essentials::severe(&say("connectionFailed"));
                return;
            };
            body = database.to_vec();
        }
    }
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::write(file, &body).is_err() {
        essentials::severe(&say("connectionFailed"));
    }
}

/// `reloadConfig`, past the old `.dat` files of a version long gone.
fn open_database(config: &Config) -> (Option<Database>, Vec<String>) {
    let file = folder().join(if config.get_boolean("database.show-cities", false) { "GeoIP2-City.mmdb" } else { "GeoIP2-Country.mmdb" });
    if !file.exists() {
        if !config.get_boolean("database.download-if-missing", true) {
            essentials::severe(&say("cantFindGeoIpDB"));
            return (None, Vec::new());
        }
        download_database(config, &file);
    } else if config.get_boolean("database.update.enable", true) {
        // Past its age, a new one is fetched.
        let days = file.metadata().and_then(|meta| meta.modified()).ok().and_then(|modified| modified.elapsed().ok()).map_or(0, |age| age.as_secs() / 86_400);
        if days > config.get_long("database.update.by-every-x-days", 30).max(0) as u64 {
            download_database(config, &file);
        }
    }
    let mut locales = vec!["en".to_owned()];
    if config.get_boolean("enable-locale", false) {
        let mut locale = current_locale().name().replace('_', "-");
        if locale.eq_ignore_ascii_case("zh") {
            locale = "zh-CN".to_owned();
        }
        locales.insert(0, locale);
    }
    let database = std::fs::read(&file).ok().and_then(Database::open);
    if database.is_none() {
        essentials::severe(&say("cantReadGeoIpDB"));
    }
    (database, locales)
}

/// `getName()`: the first of the reader's locales the record has.
fn name_of(field: Option<&Field>, locales: &[String]) -> Option<String> {
    let names = field?.get("names")?;
    locales.iter().find_map(|locale| match names.get(locale) {
        Some(Field::Text(name)) => Some(name.clone()),
        _ => None,
    })
}

fn place_of(record: &Field, cities: bool, locales: &[String]) -> String {
    let mut parts = Vec::new();
    if cities {
        parts.extend(name_of(record.get("city"), locales));
        let last = match record.get("subdivisions") {
            Some(Field::List(list)) => list.last(),
            _ => None,
        };
        parts.extend(name_of(last, locales));
    }
    parts.push(name_of(record.get("country"), locales).unwrap_or_else(|| "null".to_owned()));
    parts.join(", ")
}

/// `checkIfLocal`.
fn is_local(address: IpAddr) -> bool {
    address.is_loopback() || address.is_unspecified()
}

fn delayed_join(player: &Player, user: &User) {
    let Some(text) = player.get_address() else { return };
    if user.is_authorized("essentials.geoip.hide") {
        return;
    }
    let Ok(address) = text.parse::<IpAddr>() else { return };
    let state = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(reader) = &state.reader else {
        essentials::warning(&mini_to_legacy(&tl_literal("geoIpErrorOnJoin", essentials::args![user.get_name()])));
        return;
    };
    let cities = state.config.get_boolean("database.show-cities", false);
    let Some(record) = reader.lookup(address) else {
        if is_local(address) {
            for other in ess().get_online_users() {
                if other.is_authorized("essentials.geoip.show") {
                    other.send_tl("geoipCantFind", essentials::args![user.get_display_name()]);
                }
            }
        } else {
            essentials::info(&say("cantReadGeoIpDB"));
        }
        return;
    };
    let place = place_of(&record, cities, &state.locales);
    if state.config.get_boolean("show-on-whois", true) {
        user.set_geo_location(Some(place.clone()));
    }
    if state.config.get_boolean("show-on-login", true) && !user.is_hidden() {
        essentials::info(&mini_to_legacy(&tl_literal("geoipJoinFormat", essentials::args![user.get_display_name(), place.clone()])));
        for other in ess().get_online_users() {
            if other.is_authorized("essentials.geoip.show") {
                other.send_tl("geoipJoinFormat", essentials::args![user.get_display_name(), place.clone()]);
            }
        }
    }
}

/// `onEnable`.
pub fn on_enable(plugin: Plugin) {
    let _ = std::fs::create_dir_all(folder());
    LazyLock::force(&STATE);
    plugin.register_event_with(horizon_bukkit::enums::event_priority::MONITOR, false, |event: &mut PlayerJoinEvent| {
        let player = event.get_player().clone();
        let mut task = Some(player);
        ess().schedule_sync_delayed_task_later(
            move || {
                if let Some(player) = task.take()
                    && let Some(user) = ess().get_user(&player)
                {
                    delayed_join(&player, &user);
                }
            },
            1,
        );
    });
    plugin.info("This product includes GeoLite2 data created by MaxMind, available from http://www.maxmind.com/.");
}
