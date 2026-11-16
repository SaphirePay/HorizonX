//! `PasteUtil`: an anonymous paste on pastes.dev. Runs on the caller's thread: every caller is off the
//! main one.
use serde_json::json;
use std::time::Duration;

const PASTE_URL: &str = "https://pastes.dev/";
const PASTE_UPLOAD_URL: &str = "https://api.pastes.dev/post";

/// The paste's id and address, or what went wrong.
pub fn create_paste(files: &[(String, String)]) -> Result<(String, String), String> {
    let files: Vec<_> = files.iter().map(|(name, contents)| json!({ "name": name, "content": { "format": "text", "value": contents } })).collect();
    let body = json!({ "files": files }).to_string();
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).user_agent("EssentialsX plugin").build();
    let answer = agent.post(PASTE_UPLOAD_URL).set("Content-Type", "text/json").send_string(&body).map_err(|error| error.to_string())?;
    let answer: serde_json::Value = answer.into_json().map_err(|error| error.to_string())?;
    let id = answer["key"].as_str().ok_or_else(|| "no key in the answer".to_owned())?.to_owned();
    let url = format!("{PASTE_URL}{id}");
    Ok((id, url))
}
