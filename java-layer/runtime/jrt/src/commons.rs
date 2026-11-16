//! Apache Commons Lang's `StringUtils` and `WordUtils`, which the server ships and plugins call
//! without shading: the null-safe text helpers.
use crate::{R, V, s};

fn text(v: Option<&V>) -> Option<String> {
    match v {
        None | Some(V::Null) => None,
        Some(v) => Some(v.text()),
    }
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

fn uncapitalize(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |first| first.to_lowercase().chain(chars).collect())
}

pub fn static_call(class: &str, name: &str, a: &[V]) -> Option<R> {
    let simple = class.rsplit('.').next().unwrap_or(class);
    if !matches!(simple, "StringUtils" | "WordUtils") {
        return None;
    }
    let first = text(a.first());
    let or_null = |f: &dyn Fn(&str) -> String| first.as_deref().map_or(V::Null, |t| s(&f(t)));
    Some(Ok(match (simple, name) {
        ("StringUtils", "isEmpty") => V::Bool(first.as_deref().is_none_or(str::is_empty)),
        ("StringUtils", "isNotEmpty") => V::Bool(!first.as_deref().is_none_or(str::is_empty)),
        ("StringUtils", "isBlank") => V::Bool(first.as_deref().is_none_or(|t| t.trim().is_empty())),
        ("StringUtils", "isNotBlank") => V::Bool(!first.as_deref().is_none_or(|t| t.trim().is_empty())),
        ("StringUtils", "capitalize") => or_null(&capitalize),
        ("StringUtils", "uncapitalize") => or_null(&uncapitalize),
        ("StringUtils", "upperCase") => or_null(&|t| t.to_uppercase()),
        ("StringUtils", "lowerCase") => or_null(&|t| t.to_lowercase()),
        ("StringUtils", "trim" | "strip") => or_null(&|t| t.trim().to_owned()),
        ("StringUtils", "reverse") => or_null(&|t| t.chars().rev().collect()),
        ("StringUtils", "defaultString") => s(&first.unwrap_or_else(|| text(a.get(1)).unwrap_or_default())),
        ("StringUtils", "defaultIfEmpty" | "defaultIfBlank") => match first {
            Some(t) if !(if name == "defaultIfBlank" { t.trim().is_empty() } else { t.is_empty() }) => s(&t),
            _ => a.get(1).cloned().unwrap_or(V::Null),
        },
        ("StringUtils", "repeat") => match (&first, a.len()) {
            (Some(t), 2) => s(&t.repeat(a[1].int().max(0) as usize)),
            (Some(t), 3) => s(&vec![t.as_str(); a[2].int().max(0) as usize].join(&a[1].text())),
            _ => V::Null,
        },
        ("StringUtils", "equals") => V::Bool(first == text(a.get(1))),
        ("StringUtils", "equalsIgnoreCase") => V::Bool(first.map(|t| t.to_lowercase()) == text(a.get(1)).map(|t| t.to_lowercase())),
        ("StringUtils", "contains") => V::Bool(matches!((&first, text(a.get(1))), (Some(t), Some(p)) if t.contains(p.as_str()))),
        ("StringUtils", "containsIgnoreCase") => V::Bool(matches!((&first, text(a.get(1))), (Some(t), Some(p)) if t.to_lowercase().contains(&p.to_lowercase()))),
        ("StringUtils", "startsWith") => V::Bool(matches!((&first, text(a.get(1))), (Some(t), Some(p)) if t.starts_with(p.as_str()))),
        ("StringUtils", "endsWith") => V::Bool(matches!((&first, text(a.get(1))), (Some(t), Some(p)) if t.ends_with(p.as_str()))),
        ("StringUtils", "isNumeric") => V::Bool(first.as_deref().is_some_and(|t| !t.is_empty() && t.chars().all(char::is_numeric))),
        ("StringUtils", "isAlpha") => V::Bool(first.as_deref().is_some_and(|t| !t.is_empty() && t.chars().all(char::is_alphabetic))),
        ("StringUtils", "countMatches") => V::Int(match (&first, text(a.get(1))) {
            (Some(t), Some(p)) if !p.is_empty() => t.matches(p.as_str()).count() as i32,
            _ => 0,
        }),
        ("StringUtils", "replace") => match (&first, text(a.get(1)), text(a.get(2))) {
            (Some(t), Some(from), Some(to)) if !from.is_empty() => s(&t.replace(from.as_str(), &to)),
            _ => a.first().cloned().unwrap_or(V::Null),
        },
        ("StringUtils", "join") => {
            let items = match a.first() {
                Some(V::Arr(_) | V::List(_) | V::Set(_)) => a[0].items().unwrap_or_default(),
                _ => return None,
            };
            s(&items.iter().map(V::text).collect::<Vec<_>>().join(&text(a.get(1)).unwrap_or_default()))
        }
        ("StringUtils", "left") => or_null(&|t| t.chars().take(a.get(1).map_or(0, V::int).max(0) as usize).collect()),
        ("StringUtils", "right") => or_null(&|t| {
            let count = t.chars().count();
            t.chars().skip(count.saturating_sub(a.get(1).map_or(0, V::int).max(0) as usize)).collect()
        }),
        ("WordUtils", "capitalize" | "capitalizeFully") => or_null(&|t| {
            t.split(' ').map(|w| if name == "capitalizeFully" { capitalize(&w.to_lowercase()) } else { capitalize(w) }).collect::<Vec<_>>().join(" ")
        }),
        ("WordUtils", "uncapitalize") => or_null(&|t| t.split(' ').map(uncapitalize).collect::<Vec<_>>().join(" ")),
        _ => return None,
    }))
}
