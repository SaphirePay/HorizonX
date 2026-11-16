//! The bot token, webhook tokens and interaction tokens. None of them may reach a log, on success or
//! on failure, so a `Secret` doesn't print, and `scrub` takes out of a line whatever got that far.
use regex::Regex;
use std::fmt;
use std::sync::{Arc, LazyLock, Mutex};

/// A token. `expose` is for the header or path it goes in, and nothing else.
#[derive(Clone)]
pub struct Secret(Arc<str>);

static KNOWN: Mutex<Vec<Arc<str>>> = Mutex::new(Vec::new());

impl Secret {
    pub fn new(value: &str) -> Self {
        let value: Arc<str> = Arc::from(value.trim());
        if !value.is_empty() {
            let mut known = KNOWN.lock().unwrap_or_else(|e| e.into_inner());
            if !known.iter().any(|seen| *seen == value) {
                known.push(value.clone());
            }
        }
        Self(value)
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

/// A `Secret` inside something that compares: two are the same if their tokens are.
#[derive(Clone, Debug)]
pub struct SecretEq(pub Secret);

impl PartialEq for SecretEq {
    fn eq(&self, other: &Self) -> bool {
        self.0.expose() == other.0.expose()
    }
}

impl Eq for SecretEq {}

static WEBHOOK_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(webhooks/\d+/)[\w.-]+").expect("regex"));
static INTERACTION_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(interactions/\d+/)[\w.-]+").expect("regex"));

/// `text` with every token this process has seen, and anything shaped like a token's URL, taken out.
pub fn scrub(text: &str) -> String {
    let mut out = text.to_owned();
    for known in KNOWN.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        if out.contains(&**known) {
            out = out.replace(&**known, "***");
        }
    }
    let out = WEBHOOK_URL.replace_all(&out, "${1}***");
    INTERACTION_URL.replace_all(&out, "${1}***").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_does_not_print() {
        let secret = Secret::new("abc.def.ghi");
        assert_eq!(format!("{secret:?}"), "Secret(***)");
        assert_eq!(secret.expose(), "abc.def.ghi");
    }

    #[test]
    fn scrub_takes_out_tokens_and_urls() {
        let _secret = Secret::new("my-very-own-token");
        assert_eq!(scrub("failed with my-very-own-token!"), "failed with ***!");
        assert_eq!(scrub("https://discord.com/api/v10/webhooks/123/aBc-d_E.f?wait=false"), "https://discord.com/api/v10/webhooks/123/***?wait=false");
        assert_eq!(scrub("POST /interactions/9/secret.part/callback"), "POST /interactions/9/***/callback");
    }
}
