//! Message formatting the way Java plugins write it: `String.format` (`%s`, `%d`, `%.2f`, `%1$s`, `%%`) and
//! `MessageFormat`-style `{0}`.

/// Java's `String.format` for `%s`, `%d`, `%b`, `%x`, `%.Nf`, `%1$s` and `%%`.
pub fn java(text: &str, args: &[&str]) -> String {
    if !text.contains('%') {
        return text.to_owned();
    }
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut next = 0;
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' && i + 1 < chars.len() {
            if chars[i + 1] == '%' {
                out.push('%');
                i += 2;
                continue;
            }
            let mut j = i + 1;
            let mut index = None;
            let digits: String = chars[j..].iter().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() && chars.get(j + digits.len()) == Some(&'$') {
                index = digits.parse::<usize>().ok().map(|n| n.saturating_sub(1));
                j += digits.len() + 1;
            }
            let spec: String = chars[j..].iter().take_while(|c| !c.is_ascii_alphabetic() && **c != '%' && **c != ' ').collect();
            j += spec.len();
            if let Some(conv) = chars.get(j).copied().filter(|c| "sdbfxS".contains(*c)) {
                let n = index.unwrap_or_else(|| {
                    next += 1;
                    next - 1
                });
                let v = args.get(n).copied().unwrap_or_default().to_owned();
                let v = match (conv, spec.strip_prefix('.').and_then(|p| p.parse::<usize>().ok())) {
                    ('f', Some(p)) => v.parse::<f64>().map_or(v.clone(), |f| format!("{f:.p$}")),
                    ('x', _) => v.parse::<i64>().map_or(v.clone(), |n| format!("{n:x}")),
                    ('S', _) => v.to_uppercase(),
                    _ => v,
                };
                out.push_str(&v);
                i = j + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// `{0}`, `{1}` ... filled in (MessageFormat style; other braces left).
pub fn braces(text: &str, args: &[&str]) -> String {
    if !text.contains('{') {
        return text.to_owned();
    }
    let mut out = text.to_owned();
    for (i, a) in args.iter().enumerate() {
        out = out.replace(&format!("{{{i}}}"), a);
    }
    out
}

/// Named placeholders: `{player}`, `%player%` -> value.
pub fn named(text: &str, pairs: &[(&str, &str)]) -> String {
    let mut out = text.to_owned();
    for (k, v) in pairs {
        out = out.replace(&format!("{{{k}}}"), v).replace(&format!("%{k}%"), v);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats() {
        assert_eq!(super::java("%s has %d (%.1f%%)", &["a", "3", "2.25"]), "a has 3 (2.2%)".replace("2.2%", "2.2%"));
        assert_eq!(super::java("%2$s %1$s", &["a", "b"]), "b a");
        assert_eq!(super::braces("{0} and {1}", &["x", "y"]), "x and y");
        assert_eq!(super::named("Hi {player} %player%", &[("player", "Bo")]), "Hi Bo Bo");
    }
}
