//! More of what the 200-plugin sweep reached for and jrt didn't have (missing_report.py's "stopped a plugin as it
//! ran" list): `Locale`, Guava's `Queues`, `Scanner`, the `Atomic*FieldUpdater`s, `TimeZone`, `System.getLogger`,
//! json-simple's `JSONValue`, `String.getChars` and `Class.getTypeParameters`.
use crate::{R, V, arr, exc, lock, native, s};
use std::sync::Mutex;

// ---- Locale: its name ("en_US"), as `Locale.toString` writes it ----

fn locale(language: &str, country: &str, variant: &str) -> V {
    let mut name = language.to_ascii_lowercase();
    if !country.is_empty() || !variant.is_empty() {
        name.push('_');
        name.push_str(&country.to_ascii_uppercase());
    }
    if !variant.is_empty() {
        name.push('_');
        name.push_str(variant);
    }
    s(&name)
}

/// "", "en", "en_US", "zh_TW", "sr_RS_latin".
fn looks_like_locale(text: &str) -> bool {
    let mut parts = text.split('_');
    let language = parts.next().unwrap_or("");
    let country = parts.next().unwrap_or("");
    language.len() <= 3 && language.bytes().all(|b| b.is_ascii_lowercase()) && country.len() <= 3 && country.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

fn locale_part(name: &str, index: usize) -> String {
    name.split('_').nth(index).unwrap_or("").to_owned()
}

const LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"), ("de", "German"), ("fr", "French"), ("es", "Spanish"), ("it", "Italian"), ("pt", "Portuguese"),
    ("nl", "Dutch"), ("ru", "Russian"), ("pl", "Polish"), ("tr", "Turkish"), ("ja", "Japanese"), ("zh", "Chinese"),
    ("ko", "Korean"), ("sv", "Swedish"), ("cs", "Czech"), ("uk", "Ukrainian"), ("fi", "Finnish"), ("da", "Danish"),
];

fn locale_method(name: &str, method: &str) -> Option<R> {
    let language = locale_part(name, 0);
    let country = locale_part(name, 1);
    Some(Ok(match method {
        "getLanguage" => s(&language),
        "getCountry" => s(&country),
        "getVariant" => s(&locale_part(name, 2)),
        "toLanguageTag" => s(&if language.is_empty() { "und".to_owned() } else if country.is_empty() { language } else { format!("{language}-{country}") }),
        "getDisplayLanguage" => s(LANGUAGES.iter().find(|(code, _)| *code == language).map_or(language.as_str(), |(_, shown)| shown)),
        "getDisplayName" => {
            let shown = LANGUAGES.iter().find(|(code, _)| *code == language).map_or(language.clone(), |(_, shown)| (*shown).to_owned());
            s(&if country.is_empty() { shown } else { format!("{shown} ({country})") })
        }
        "getISO3Language" => s(&language),
        _ => return None,
    }))
}

// ---- Scanner ----

pub struct JScanner(Mutex<ScanState>);

struct ScanState {
    text: Vec<char>,
    at: usize,
    /// `useDelimiter`: none is whitespace.
    delimiter: Option<fancy_regex::Regex>,
}

impl ScanState {
    /// The next token and where reading goes on after it. Whitespace, the default, runs together; a
    /// delimiter of the plugin's own gives an empty token between two of it (`"a,,b"` is a, "", b), as Java's does.
    fn peek_token(&mut self) -> Option<(String, usize)> {
        match &self.delimiter {
            None => {
                while self.at < self.text.len() && self.text[self.at].is_whitespace() {
                    self.at += 1;
                }
                if self.at >= self.text.len() {
                    return None;
                }
                let mut end = self.at;
                while end < self.text.len() && !self.text[end].is_whitespace() {
                    end += 1;
                }
                Some((self.text[self.at..end].iter().collect(), end))
            }
            Some(pattern) => {
                if self.at >= self.text.len() {
                    return None;
                }
                let rest: String = self.text[self.at..].iter().collect();
                let found = pattern.find_iter(&rest).filter_map(Result::ok).find(|m| m.end() > m.start());
                Some(match found {
                    Some(m) => (rest[..m.start()].to_owned(), self.at + rest[..m.end()].chars().count()),
                    None => (rest, self.text.len()),
                })
            }
        }
    }
}

fn scanner_text(source: &V) -> String {
    match source {
        V::Str(text) => text.to_string(),
        V::Null => String::new(),
        other => {
            if let Some(bytes) = crate::javaio::bytes_of(other) {
                return String::from_utf8_lossy(&bytes).into_owned();
            }
            // A `File` or a `Path`: what's in it.
            crate::call_static("Files", "readString", &[crate::call(other, "toPath", &[]).unwrap_or_else(|_| other.clone())]).map(|t| t.text()).unwrap_or_default()
        }
    }
}

fn scanner_method(scanner: &JScanner, this: &V, name: &str, a: &[V]) -> Option<R> {
    let mut state = lock(&scanner.0);
    let no_element = || Err(exc("NoSuchElementException", "No line found"));
    Some(match name {
        "hasNextLine" => Ok(V::Bool(state.at < state.text.len())),
        "nextLine" => {
            if state.at >= state.text.len() {
                return Some(no_element());
            }
            let start = state.at;
            let mut end = start;
            while end < state.text.len() && state.text[end] != '\n' {
                end += 1;
            }
            let line: String = state.text[start..end].iter().collect();
            state.at = end + 1;
            Ok(s(line.strip_suffix('\r').unwrap_or(&line)))
        }
        "hasNext" => Ok(V::Bool(state.peek_token().is_some())),
        "next" => match state.peek_token() {
            Some((token, end)) => {
                state.at = end;
                Ok(s(&token))
            }
            None => no_element(),
        },
        "hasNextInt" | "hasNextLong" => Ok(V::Bool(state.peek_token().is_some_and(|(t, _)| t.parse::<i64>().is_ok()))),
        "hasNextDouble" => Ok(V::Bool(state.peek_token().is_some_and(|(t, _)| t.parse::<f64>().is_ok()))),
        "hasNextBoolean" => Ok(V::Bool(state.peek_token().is_some_and(|(t, _)| t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("false")))),
        "nextInt" | "nextLong" | "nextDouble" | "nextBoolean" => match state.peek_token() {
            Some((token, end)) => {
                let value = match name {
                    "nextInt" => token.parse::<i32>().ok().map(V::Int),
                    "nextLong" => token.parse::<i64>().ok().map(V::Long),
                    "nextDouble" => token.parse::<f64>().ok().map(V::Double),
                    _ => (token.eq_ignore_ascii_case("true") || token.eq_ignore_ascii_case("false")).then(|| V::Bool(token.eq_ignore_ascii_case("true"))),
                };
                match value {
                    Some(value) => {
                        state.at = end;
                        Ok(value)
                    }
                    None => Err(exc("InputMismatchException", format!("For input string: \"{token}\""))),
                }
            }
            None => no_element(),
        },
        "useDelimiter" => {
            state.delimiter = fancy_regex::Regex::new(&a[0].text()).ok();
            Ok(this.clone())
        }
        "close" => Ok(V::Null),
        _ => return None,
    })
}

// ---- Atomic*FieldUpdater: a field of a plugin object, read and changed under the object's lock ----

pub struct JFieldUpdater {
    field: &'static str,
    number: bool,
}

fn same_value(a: &V, b: &V, number: bool) -> bool {
    if number { a.long() == b.long() } else { crate::same(a, b) }
}

fn updater_method(updater: &JFieldUpdater, name: &str, a: &[V]) -> Option<R> {
    let V::Obj(object) = a.first()? else {
        return Some(Err(exc("NullPointerException", format!("{}: no object", updater.field))));
    };
    let number = updater.number;
    let zero = || if number { V::Long(0) } else { V::Null };
    let mut fields = lock(&object.fields);
    let now = fields.get(updater.field).cloned().unwrap_or_else(zero);
    let add = |by: i64| match &now {
        V::Int(n) => V::Int(n.wrapping_add(by as i32)),
        other => V::Long(other.long().wrapping_add(by)),
    };
    let (new, answer) = match (name, a.len()) {
        ("get", 1) => return Some(Ok(now)),
        ("set" | "lazySet" | "setPlain" | "setRelease", 2) => (a[1].clone(), V::Null),
        ("getAndSet", 2) => (a[1].clone(), now.clone()),
        ("compareAndSet" | "weakCompareAndSet" | "weakCompareAndSetPlain" | "weakCompareAndSetVolatile", 3) => {
            if !same_value(&now, &a[1], number) {
                return Some(Ok(V::Bool(false)));
            }
            (a[2].clone(), V::Bool(true))
        }
        ("incrementAndGet", 1) => (add(1), add(1)),
        ("decrementAndGet", 1) => (add(-1), add(-1)),
        ("getAndIncrement", 1) => (add(1), now.clone()),
        ("getAndDecrement", 1) => (add(-1), now.clone()),
        ("addAndGet", 2) => (add(a[1].long()), add(a[1].long())),
        ("getAndAdd", 2) => (add(a[1].long()), now.clone()),
        ("updateAndGet" | "getAndUpdate", 2) => {
            drop(fields);
            let new = match crate::call(&a[1], "$invoke", &[now.clone()]) {
                Ok(new) => new,
                Err(e) => return Some(Err(e)),
            };
            lock(&object.fields).insert(updater.field, new.clone());
            return Some(Ok(if name == "updateAndGet" { new } else { now }));
        }
        ("accumulateAndGet" | "getAndAccumulate", 3) => {
            drop(fields);
            let new = match crate::call(&a[2], "$invoke", &[now.clone(), a[1].clone()]) {
                Ok(new) => new,
                Err(e) => return Some(Err(e)),
            };
            lock(&object.fields).insert(updater.field, new.clone());
            return Some(Ok(if name == "accumulateAndGet" { new } else { now }));
        }
        _ => return None,
    };
    fields.insert(updater.field, new);
    Some(Ok(answer))
}

// ---- TimeZone: its id ----

pub struct JTimeZone(String);

/// "UTC", "GMT", "Z", "GMT+2", "UTC-05:30": the offset in milliseconds; a named zone reads as UTC.
fn zone_offset_ms(id: &str) -> i64 {
    let rest = id.trim_start_matches("GMT").trim_start_matches("UTC");
    let Some(sign) = rest.chars().next().filter(|c| *c == '+' || *c == '-') else { return 0 };
    let digits = &rest[1..];
    let (hours, minutes) = digits.split_once(':').map_or_else(
        || if digits.len() > 2 { (&digits[..digits.len() - 2], &digits[digits.len() - 2..]) } else { (digits, "0") },
        |(h, m)| (h, m),
    );
    let ms = hours.parse::<i64>().unwrap_or(0) * 3_600_000 + minutes.parse::<i64>().unwrap_or(0) * 60_000;
    if sign == '-' { -ms } else { ms }
}

fn zone_method(zone: &JTimeZone, name: &str) -> Option<R> {
    Some(Ok(match name {
        "getID" | "getDisplayName" | "toString" => s(&zone.0),
        "getRawOffset" | "getOffset" => V::Int(zone_offset_ms(&zone.0) as i32),
        "useDaylightTime" | "inDaylightTime" | "observesDaylightTime" => V::Bool(false),
        "getDSTSavings" => V::Int(0),
        "toZoneId" => return Some(crate::call_static("ZoneId", "of", &[s(&zone.0)])),
        _ => return None,
    }))
}

// ---- System.getLogger: a System.Logger that writes to the plugin's log ----

pub struct JSysLogger(String);

fn sys_logger_method(logger: &JSysLogger, name: &str, a: &[V]) -> Option<R> {
    match name {
        "getName" => Some(Ok(s(&logger.0))),
        "isLoggable" => Some(Ok(V::Bool(!matches!(a.first().map(V::text).as_deref(), Some("TRACE" | "DEBUG" | "OFF"))))),
        "log" if !a.is_empty() => {
            let level = match a[0].text().as_str() {
                "ERROR" => "SEVERE",
                "WARNING" => "WARNING",
                "TRACE" | "DEBUG" | "ALL" => "FINE",
                "OFF" => return Some(Ok(V::Null)),
                _ => "INFO",
            };
            let mut message = a.get(1).map(V::text).unwrap_or_default();
            // `log(level, "x {0} y", args...)`: MessageFormat's places.
            let args: Vec<V> = match a.get(2) {
                Some(V::Arr(items)) => lock(items).clone(),
                Some(V::Exc(_)) | None => Vec::new(),
                Some(_) => a[2..].to_vec(),
            };
            for (i, arg) in args.iter().enumerate() {
                message = message.replace(&format!("{{{i}}}"), &arg.text());
            }
            let bukkit = crate::call_static("Bukkit", "getLogger", &[]).unwrap_or(V::Null);
            let mut forwarded = vec![s(level), s(&message)];
            if let Some(thrown @ V::Exc(_)) = a.get(2) {
                forwarded.push(thrown.clone());
            }
            Some(crate::call(&bukkit, "log", &forwarded))
        }
        _ => None,
    }
}

// ---- The hooks jrt calls ----

pub fn construct(class: &str, a: &[V]) -> Option<R> {
    Some(Ok(match class {
        "Locale" => locale(&a.first().map(V::text).unwrap_or_default(), &a.get(1).map(V::text).unwrap_or_default(), &a.get(2).map(V::text).unwrap_or_default()),
        "Scanner" => native(JScanner(Mutex::new(ScanState { text: scanner_text(a.first().unwrap_or(&V::Null)).chars().collect(), at: 0, delimiter: None }))),
        "SimpleTimeZone" => native(JTimeZone(a.get(1).map(V::text).unwrap_or_else(|| "UTC".to_owned()))),
        _ => return None,
    }))
}

pub fn field(class: &str, name: &str) -> Option<V> {
    if class != "Locale" {
        return None;
    }
    Some(match name {
        "ROOT" => s(""),
        "ENGLISH" => s("en"),
        "US" => s("en_US"),
        "UK" => s("en_GB"),
        "CANADA" => s("en_CA"),
        "GERMAN" => s("de"),
        "GERMANY" => s("de_DE"),
        "FRENCH" => s("fr"),
        "FRANCE" => s("fr_FR"),
        "ITALIAN" => s("it"),
        "ITALY" => s("it_IT"),
        "JAPANESE" => s("ja"),
        "JAPAN" => s("ja_JP"),
        "KOREAN" => s("ko"),
        "KOREA" => s("ko_KR"),
        "CHINESE" => s("zh"),
        "CHINA" | "PRC" | "SIMPLIFIED_CHINESE" => s("zh_CN"),
        "TAIWAN" | "TRADITIONAL_CHINESE" => s("zh_TW"),
        _ => return None,
    })
}

pub fn static_call(class: &str, name: &str, a: &[V]) -> Option<R> {
    let first = || a.first().map(V::text).unwrap_or_default();
    Some(match (class, name) {
        ("Locale", "of") => Ok(locale(&first(), &a.get(1).map(V::text).unwrap_or_default(), &a.get(2).map(V::text).unwrap_or_default())),
        ("Locale", "forLanguageTag") => {
            let tag = first();
            let mut parts = tag.split(['-', '_']);
            Ok(locale(parts.next().unwrap_or(""), parts.next().unwrap_or(""), ""))
        }
        ("Locale", "getAvailableLocales") => Ok(arr(["en_US", "en_GB", "de_DE", "fr_FR", "es_ES", "it_IT", "pt_BR", "nl_NL", "ru_RU", "pl_PL", "ja_JP", "zh_CN"].iter().map(|n| s(n)).collect())),
        ("Queues", "newArrayDeque" | "newConcurrentLinkedQueue" | "newLinkedBlockingQueue" | "newLinkedBlockingDeque" | "newPriorityQueue" | "newPriorityBlockingQueue" | "newSynchronousQueue") => {
            let from: Vec<V> = a.first().filter(|v| !matches!(v, V::Int(_))).map_or_else(|| Ok(Vec::new()), V::items).unwrap_or_default();
            Ok(crate::list(from))
        }
        ("Queues", "newArrayBlockingQueue") => Ok(crate::list(Vec::new())),
        ("AtomicReferenceFieldUpdater", "newUpdater") | ("AtomicLongFieldUpdater" | "AtomicIntegerFieldUpdater", "newUpdater") => {
            let field = a.last().map(V::text).unwrap_or_default();
            Ok(native(JFieldUpdater { field: Box::leak(field.into_boxed_str()), number: class != "AtomicReferenceFieldUpdater" }))
        }
        ("TimeZone", "getTimeZone") => {
            let id = match a.first() {
                Some(zone) if zone.native::<JTimeZone>().is_some() => zone.native::<JTimeZone>().map(|z| z.0.clone()).unwrap_or_default(),
                Some(other) => crate::call(other, "getId", &[]).map(|id| id.text()).unwrap_or_else(|_| other.text()),
                None => "UTC".to_owned(),
            };
            Ok(native(JTimeZone(id)))
        }
        ("TimeZone", "getDefault") => Ok(native(JTimeZone("UTC".to_owned()))),
        ("TimeZone", "getAvailableIDs") => Ok(arr(["UTC", "GMT", "Europe/London", "Europe/Berlin", "America/New_York", "America/Los_Angeles", "Asia/Tokyo"].iter().map(|n| s(n)).collect())),
        ("System", "getLogger") => Ok(native(JSysLogger(first()))),
        // `Arrays.sort(a)`, `(a, comparator)`, `(a, from, to)`, `(a, from, to, comparator)`: in place, stable.
        ("Arrays", "sort" | "parallelSort") if !a.is_empty() => {
            let V::Arr(items) = &a[0] else {
                return Some(if a[0].is_null() { Err(exc("NullPointerException", "Arrays.sort of null")) } else { Ok(V::Null) });
            };
            let mut items = lock(items);
            let ranged = a.len() >= 3 && matches!(a[1], V::Int(_) | V::Long(_));
            let (from, to) = if ranged { (a[1].int().max(0) as usize, a[2].int().max(0) as usize) } else { (0, items.len()) };
            if from > to || to > items.len() {
                return Some(Err(exc("ArrayIndexOutOfBoundsException", format!("from {from}, to {to}, length {}", items.len()))));
            }
            let comparator = if ranged { a.get(3) } else { a.get(1) };
            let mut part = items[from..to].to_vec();
            drop(items);
            crate::stdlib::sort_values(&mut part, comparator);
            let V::Arr(items) = &a[0] else { unreachable!() };
            lock(items)[from..to].clone_from_slice(&part);
            Ok(V::Null)
        }
        ("JSONValue", "parse" | "parseWithException") => {
            let parser = crate::stdlib::new_builtin("JSONParser", &[])?.ok()?;
            crate::call(&parser, "parse", &a[..1])
        }
        ("JSONValue", "toJSONString") => crate::call(&a[0], "toJSONString", &[]).or_else(|_| Ok(s(&a[0].text()))),
        _ => return None,
    })
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if let Some(scanner) = this.native::<JScanner>() {
        return scanner_method(scanner, this, name, a);
    }
    if let Some(updater) = this.native::<JFieldUpdater>() {
        return updater_method(updater, name, a);
    }
    if let Some(zone) = this.native::<JTimeZone>() {
        return zone_method(zone, name);
    }
    if let Some(logger) = this.native::<JSysLogger>() {
        return sys_logger_method(logger, name, a);
    }
    match this {
        // A Locale is its name ("en", "en_US"): what Java's String hasn't got is the Locale's.
        V::Str(text) if a.is_empty() && looks_like_locale(text) => locale_method(text, name),
        // `text.getChars(begin, end, chars, at)`.
        V::Str(text) if name == "getChars" && a.len() == 4 => {
            let V::Arr(target) = &a[2] else { return Some(Err(exc("NullPointerException", "getChars: no array"))) };
            let chars: Vec<char> = text.encode_utf16().map(|u| char::from_u32(u32::from(u)).unwrap_or('\u{fffd}')).collect();
            let (begin, end, at) = (a[0].int().max(0) as usize, a[1].int().max(0) as usize, a[3].int().max(0) as usize);
            if begin > end || end > chars.len() {
                return Some(Err(exc("StringIndexOutOfBoundsException", format!("begin {begin}, end {end}, length {}", chars.len()))));
            }
            let mut target = lock(target);
            if at + (end - begin) > target.len() {
                return Some(Err(exc("ArrayIndexOutOfBoundsException", format!("{}", at + (end - begin)))));
            }
            for (i, c) in chars[begin..end].iter().enumerate() {
                target[at + i] = V::Char(*c);
            }
            Some(Ok(V::Null))
        }
        V::Class(_) if name == "getTypeParameters" && a.is_empty() => Some(Ok(arr(Vec::new()))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(r: Option<R>) -> V {
        match r {
            Some(Ok(v)) => v,
            Some(Err(e)) => panic!("threw {}", e.text()),
            None => panic!("no value"),
        }
    }

    #[test]
    fn a_locale_reads_its_parts() {
        let us = ok(construct("Locale", &[s("EN"), s("us")]));
        assert_eq!(us.text(), "en_US");
        assert_eq!(ok(method(&us, "getCountry", &[])).text(), "US");
        assert_eq!(ok(method(&us, "toLanguageTag", &[])).text(), "en-US");
        assert_eq!(ok(static_call("Locale", "forLanguageTag", &[s("de-DE")])).text(), "de_DE");
        assert_eq!(ok(method(&s("fr"), "getDisplayLanguage", &[])).text(), "French");
    }

    #[test]
    fn a_scanner_reads_lines_and_tokens() {
        let scanner = ok(construct("Scanner", &[s("first line\r\n 12  word\nlast")]));
        assert_eq!(ok(method(&scanner, "nextLine", &[])).text(), "first line");
        assert!(ok(method(&scanner, "hasNextInt", &[])).truth());
        assert_eq!(ok(method(&scanner, "nextInt", &[])).int(), 12);
        assert_eq!(ok(method(&scanner, "next", &[])).text(), "word");
        assert_eq!(ok(method(&scanner, "nextLine", &[])).text(), "");
        assert_eq!(ok(method(&scanner, "nextLine", &[])).text(), "last");
        assert!(!ok(method(&scanner, "hasNextLine", &[])).truth());
        assert!(method(&scanner, "nextLine", &[]).unwrap().is_err());
        let csv = ok(construct("Scanner", &[s("a,b,,c")]));
        ok(method(&csv, "useDelimiter", &[s(",")]));
        let tokens: Vec<String> = (0..4).map(|_| ok(method(&csv, "next", &[])).text()).collect();
        assert_eq!(tokens, ["a", "b", "", "c"]);
        assert!(!ok(method(&csv, "hasNext", &[])).truth());
    }

    #[test]
    fn a_time_zone_knows_its_offset() {
        let zone = ok(static_call("TimeZone", "getTimeZone", &[s("GMT+05:30")]));
        assert_eq!(ok(method(&zone, "getRawOffset", &[])).int(), 19_800_000);
        assert_eq!(ok(method(&zone, "getID", &[])).text(), "GMT+05:30");
        assert_eq!(zone_offset_ms("UTC-2"), -7_200_000);
        assert_eq!(zone_offset_ms("Europe/Berlin"), 0);
    }

    #[test]
    fn get_chars_copies_into_the_array() {
        let target = arr(vec![V::Char(' '); 5]);
        ok(method(&s("hello"), "getChars", &[V::Int(1), V::Int(4), target.clone(), V::Int(1)]));
        let V::Arr(items) = &target else { unreachable!() };
        let got: String = lock(items).iter().map(|c| if let V::Char(c) = c { *c } else { '?' }).collect();
        assert_eq!(got, " ell ");
    }

    #[test]
    fn arrays_sort_in_place_and_by_range() {
        let numbers = arr(vec![V::Int(3), V::Int(1), V::Int(2), V::Int(0)]);
        ok(static_call("Arrays", "sort", &[numbers.clone(), V::Int(0), V::Int(3)]));
        let V::Arr(items) = &numbers else { unreachable!() };
        assert_eq!(lock(items).iter().map(V::int).collect::<Vec<_>>(), [1, 2, 3, 0]);
        ok(static_call("Arrays", "sort", &[numbers.clone()]));
        assert_eq!(lock(items).iter().map(V::int).collect::<Vec<_>>(), [0, 1, 2, 3]);
        let words = arr(vec![s("pear"), s("apple")]);
        ok(static_call("Arrays", "sort", &[words.clone()]));
        assert_eq!(words.items().map(|w| w.iter().map(V::text).collect::<Vec<_>>()).unwrap_or_default(), ["apple", "pear"]);
    }

    #[test]
    fn queues_make_lists() {
        let queue = ok(static_call("Queues", "newArrayDeque", &[]));
        assert!(queue.items().is_ok_and(|items| items.is_empty()));
        let copied = ok(static_call("Queues", "newArrayDeque", &[crate::list(vec![V::Int(1), V::Int(2)])]));
        assert!(copied.items().is_ok_and(|items| items.len() == 2));
    }
}
