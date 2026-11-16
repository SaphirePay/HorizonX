//! `StringUtil`.

fn replace_not(text: &str, keep: impl Fn(char) -> bool) -> String {
    text.to_lowercase().chars().map(|c| if keep(c) { c } else { '_' }).collect()
}

/// `sanitizeFileName`.
pub fn sanitize_file_name(name: &str) -> String {
    replace_not(name, |c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// `isReservedFileName`.
pub fn is_reserved_file_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    if matches!(lower.as_str(), "con" | "prn" | "aux" | "nul") {
        return true;
    }
    lower.len() == 4 && (lower.starts_with("com") || lower.starts_with("lpt")) && lower.as_bytes()[3].is_ascii_digit()
}

/// `safeString`.
pub fn safe_string(text: &str) -> String {
    replace_not(text, |c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// `sanitizeString`.
pub fn sanitize_string(text: &str) -> String {
    text.chars().filter(|c| matches!(*c, '\t' | '\n' | '\r' | '\u{20}'..='\u{7e}' | '\u{85}' | '\u{a0}'..='\u{d7ff}' | '\u{e000}'..='\u{fffc}')).collect()
}

/// `joinList`: Java's overload for a collection.
pub fn join_list(list: &[String]) -> String {
    list.join(", ")
}

/// `toUUID`: `UUID.fromString`'s text, or nothing.
pub fn to_uuid(input: &str) -> Option<String> {
    let parts: Vec<&str> = input.split('-').collect();
    if parts.len() != 5 || !parts.iter().all(|part| !part.is_empty() && part.len() <= 16 && part.bytes().all(|b| b.is_ascii_hexdigit())) {
        return None;
    }
    let widths = [8, 4, 4, 4, 12];
    Some(parts.iter().zip(widths).map(|(part, width)| format!("{:0>width$}", part.to_lowercase())).collect::<Vec<_>>().join("-"))
}

/// `joinListSkip(", ", skip, list)`.
pub fn join_list_skip(skip: &str, list: &[String]) -> String {
    list.iter().filter(|each| !each.eq_ignore_ascii_case(skip)).cloned().collect::<Vec<_>>().join(", ")
}

/// `abbreviate`.
pub fn abbreviate(input: &str, length: usize) -> String {
    if input.chars().count() <= length {
        return input.to_owned();
    }
    format!("{}...", input.chars().take(length - 3).collect::<String>())
}

/// `stripToNull`.
pub fn strip_to_null(input: &str) -> Option<String> {
    Some(input.trim().to_owned()).filter(|text| !text.is_empty())
}

/// `String.split(regex)` with a one-char pattern: trailing empty strings go, as Java's do.
pub fn java_split(text: &str, separator: char) -> Vec<String> {
    let mut parts: Vec<String> = text.split(separator).map(str::to_owned).collect();
    while parts.len() > 1 && parts.last().is_some_and(String::is_empty) {
        parts.pop();
    }
    if parts.len() == 1 && parts[0].is_empty() && !text.is_empty() {
        parts.clear();
    }
    parts
}

/// `Character.toTitleCase` of the first letter, and `I18n.capitalCase`.
pub fn capital_case(input: &str) -> String {
    let mut chars = input.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str().to_lowercase()),
        None => String::new(),
    }
}

/// `String.hashCode`.
pub fn java_hash(text: &str) -> i32 {
    text.encode_utf16().fold(0_i32, |hash, unit| hash.wrapping_mul(31).wrapping_add(i32::from(unit)))
}

/// The order a Java `HashMap<String, ...>` lists its keys in, given the order they went in. Where the
/// original prints a `HashMap` as it comes, `/kit` for one, chat has to come out in this order.
pub fn java_hash_map_order(keys: impl IntoIterator<Item = String>) -> Vec<String> {
    let keys: Vec<String> = keys.into_iter().collect();
    // The table doubles past three quarters full, and a split keeps each bucket's order.
    let mut table = 16_usize;
    while keys.len() * 4 > table * 3 {
        table *= 2;
    }
    let bucket = |key: &String| {
        let hash = java_hash(key) as u32;
        ((hash ^ (hash >> 16)) as usize) & (table - 1)
    };
    let mut ordered: Vec<(usize, usize, String)> = keys.into_iter().enumerate().map(|(at, key)| (bucket(&key), at, key)).collect();
    ordered.sort_by_key(|(slot, at, _)| (*slot, *at));
    ordered.into_iter().map(|(_, _, key)| key).collect()
}

/// `UUID.nameUUIDFromBytes`: MD5, with the version and variant bits set.
pub fn name_uuid_from_bytes(name: &[u8]) -> String {
    let mut hash = md5(name);
    hash[6] = (hash[6] & 0x0f) | 0x30;
    hash[8] = (hash[8] & 0x3f) | 0x80;
    let hex: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}

fn md5(message: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21];
    let table: Vec<u32> = (0..64).map(|i| (f64::from(i + 1).sin().abs() * 4_294_967_296.0) as u32).collect();
    let mut data = message.to_vec();
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&((message.len() as u64).wrapping_mul(8)).to_le_bytes());
    let mut state: [u32; 4] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];
    for chunk in data.chunks(64) {
        let words: Vec<u32> = chunk.chunks(4).map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]])).collect();
        let [mut a, mut b, mut c, mut d] = state;
        for i in 0..64 {
            let (mix, index) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let rotated = a.wrapping_add(mix).wrapping_add(table[i]).wrapping_add(words[index]).rotate_left(S[i]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(rotated);
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
    }
    let mut out = [0; 16];
    for (at, word) in state.iter().enumerate() {
        out[at * 4..at * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_cleaned_like_java() {
        assert_eq!(safe_string("My Home!"), "my_home_");
        assert_eq!(sanitize_file_name("My-Warp 1"), "my-warp_1");
        assert!(is_reserved_file_name("COM1") && !is_reserved_file_name("home"));
    }

    #[test]
    fn split_drops_trailing_empties() {
        assert_eq!(java_split("a:b:", ':'), ["a", "b"]);
        assert_eq!(java_split("a", ':'), ["a"]);
        assert_eq!(java_split("", ':'), [""]);
        assert!(java_split(":", ':').is_empty());
    }

    #[test]
    fn name_uuids_match_java() {
        // UUID.nameUUIDFromBytes("OfflinePlayer:Notch".getBytes(UTF_8)), a value known widely.
        assert_eq!(name_uuid_from_bytes(b"OfflinePlayer:Notch"), "b50ad385-829d-3141-a216-7e7d7539ba7f");
    }

    #[test]
    fn hash_order_is_javas() {
        assert_eq!(java_hash("tools"), 110_545_371);
        // new HashMap with "tools", "starter", "vip" lists them by bucket.
        let order = java_hash_map_order(["tools", "starter", "vip"].map(str::to_owned));
        assert_eq!(order.len(), 3);
        assert_eq!(order[0], "starter");
    }

    #[test]
    fn uuids_parse_like_from_string() {
        assert_eq!(to_uuid("5F4E6A00-1111-2222-3333-444455556666").as_deref(), Some("5f4e6a00-1111-2222-3333-444455556666"));
        assert!(to_uuid("notauuid").is_none());
    }
}
