//! `UpdateChecker`, for a release build: where 2.22.0 stands against the latest release. It asks GitHub,
//! then EssentialsX's own proxy if GitHub is down, on the caller's thread: every caller is off the main one.
use crate::prelude::*;
use serde_json::Value;
use std::sync::Mutex;
use std::time::Duration;

const REPO: &str = "EssentialsX/Essentials";
const PROXY: &str = "https://webapi.essentialsx.net/api/v1/github/essx-v2";
const STALE_MS: i64 = 1_800_000;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Status {
    Identical,
    Behind(String),
    Unknown,
    Error,
}

static CACHE: Mutex<Option<(i64, Status)>> = Mutex::new(None);

/// One request: 404 is `Ok(None)`, as a build GitHub doesn't know; 500, 403 and no answer are `Err`.
fn fetch(url: &str) -> Result<Option<Value>, ()> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(10)).user_agent("EssentialsX").build();
    match agent.get(url).call() {
        Ok(response) => response.into_json().map(Some).map_err(|_| ()),
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(_) => Err(()),
    }
}

/// `tryRequestWithFallback`.
fn fetch_either(github: &str, proxy: &str) -> Result<Option<Value>, ()> {
    fetch(github).or_else(|()| fetch(proxy))
}

fn latest_status(own: &str) -> Status {
    let latest = match fetch_either(&format!("https://api.github.com/repos/{REPO}/releases/latest"), &format!("{PROXY}/releases/latest")) {
        Ok(Some(answer)) => answer["tag_name"].as_str().map(str::to_owned),
        Ok(None) => return Status::Unknown,
        Err(()) => return Status::Error,
    };
    let Some(latest) = latest else { return Status::Error };
    match fetch_either(&format!("https://api.github.com/repos/{REPO}/compare/{latest}...{own}"), &format!("{PROXY}/compare/{latest}/{own}")) {
        Ok(Some(answer)) => match answer["status"].as_str() {
            Some("identical") => Status::Identical,
            Some("behind") => Status::Behind(latest),
            _ => Status::Unknown,
        },
        Ok(None) => Status::Unknown,
        Err(()) => Status::Error,
    }
}

fn status(own: &str) -> Status {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, known)) = cache.as_ref().filter(|(when, _)| now_ms() - when < STALE_MS) {
        return known.clone();
    }
    let found = latest_status(own);
    // A failure isn't kept, as `fetchLatestRelease` doesn't cache one.
    if found != Status::Error {
        *cache = Some((now_ms(), found.clone()));
    }
    found
}

/// `getVersionMessages`, each a key and its arguments: `latest` says so when it's up to date, and
/// `verbose` tells the odd cases too.
pub fn version_messages(latest: bool, verbose: bool) -> Vec<(&'static str, Vec<Arg>)> {
    if !settings().is_update_check_enabled() {
        return vec![("versionCheckDisabled", Vec::new())];
    }
    let own = ess().plugin().get_version().to_owned();
    let build = format!("{own} (2.x)");
    match status(&own) {
        Status::Identical if latest => vec![("versionReleaseLatest", Vec::new())],
        Status::Identical => Vec::new(),
        Status::Behind(newest) => vec![("versionReleaseNew", crate::args![newest].to_vec()), ("versionReleaseNewLink", crate::args!["https://essentialsx.net/downloads.html?branch=stable"].to_vec())],
        Status::Unknown if verbose => vec![("versionCustom", crate::args![build].to_vec())],
        Status::Unknown => Vec::new(),
        Status::Error => vec![(if verbose { "versionError" } else { "versionErrorPlayer" }, crate::args![build].to_vec())],
    }
}
