//! Pure helpers: text, ids, times, materials, config edits. No game calls here (all unit-tested).

/// `UUIDUtil.mojangIdToJavaId`: 32 hex digits get their dashes; a dashed id stays as it is.
pub fn dashed(id: &str) -> String {
    let bare: String = id.trim().chars().filter(|c| *c != '-').collect();
    if bare.len() == 32 && bare.chars().all(|c| c.is_ascii_hexdigit()) {
        let b = bare.to_ascii_lowercase();
        format!("{}-{}-{}-{}-{}", &b[0..8], &b[8..12], &b[12..16], &b[16..20], &b[20..32])
    } else {
        id.trim().to_owned()
    }
}

pub const EMPTY_UUID: &str = "00000000-0000-0000-0000-000000000000";

/// `QueuedPlayer.isBlank`.
pub fn is_blank(value: Option<&str>) -> bool {
    value.is_none_or(|v| v.trim().is_empty() || v.trim().eq_ignore_ascii_case("null"))
}

/// A Bedrock XUID: all digits, or 16 hex digits.
pub fn looks_like_xuid(id: Option<&str>) -> bool {
    if is_blank(id) {
        return false;
    }
    let t = id.unwrap_or_default().trim();
    t.chars().all(|c| c.is_ascii_digit()) || (t.len() == 16 && t.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Java's `"Tebex"` -> `"§bTebex"` (`replaceAll`).
fn tebex_blue(message: &str) -> String {
    message.replace("Tebex", "§bTebex")
}

/// `Responder.formatFancy`.
pub fn fancy(console: bool, message: &str, args: &[&str]) -> String {
    let mut message = if console { message.to_owned() } else { tebex_blue(message) };
    for (i, arg) in args.iter().enumerate() {
        let holder = format!("{{{i}}}");
        if console {
            message = message.replace(&holder, arg);
        } else {
            let arg = if arg.contains("https://") { format!("§n{arg}") } else { (*arg).to_owned() };
            message = message.replace(&holder, &format!("§6{arg}§f"));
        }
    }
    if console { message } else { format!("§b[Tebex] §f{message}") }
}

/// `Responder.formatError`.
pub fn error(console: bool, message: &str) -> String {
    if console { message.to_owned() } else { format!("§b[Tebex] §c{}", tebex_blue(message)) }
}

/// `Responder.formatSuccess`.
pub fn success(console: bool, message: &str, args: &[&str]) -> String {
    if console {
        return message.to_owned();
    }
    let mut message = tebex_blue(message);
    for (i, arg) in args.iter().enumerate() {
        message = message.replace(&format!("{{{i}}}"), &format!("§6{arg}§f"));
    }
    format!("§b[Tebex] §a{message}")
}

/// `DecimalFormat("#.##")`: at most two decimals, no trailing zeros, half-even.
pub fn price(value: f64) -> String {
    let scaled = value * 100.0;
    let floor = scaled.floor();
    let frac = scaled - floor;
    let cents = if (frac - 0.5).abs() < 1e-9 { if floor % 2.0 == 0.0 { floor } else { floor + 1.0 } } else { scaled.round() };
    let cents = cents as i64;
    let (sign, cents) = if cents < 0 { ("-", -cents) } else { ("", cents) };
    let (whole, part) = (cents / 100, cents % 100);
    match part {
        0 => format!("{sign}{whole}"),
        p if p % 10 == 0 => format!("{sign}{whole}.{}", p / 10),
        p => format!("{sign}{whole}.{p:02}"),
    }
}

/// `String.format("%.2f")`.
pub fn two_places(value: f64) -> String {
    format!("{value:.2}")
}

/// `ServerEvent.anonymizeIp`: the last part of an IPv4 address becomes `x`.
pub fn anonymize_ip(ip: &str) -> String {
    match ip.rfind('.') {
        Some(i) => format!("{}.x", &ip[..i]),
        None => ip.to_owned(),
    }
}

/// Now, as `yyyy-MM-dd'T'HH:mm:ss'Z'` (UTC).
pub fn utc_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    utc_of(secs)
}

pub fn utc_of(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil(days);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Days since 1970-01-01 to a date (Howard Hinnant's algorithm).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// `[{(<\[](name|player|username)[})>\]]`, any case, every match replaced (`PlaceholderManager`).
pub fn replace_bracketed(text: &str, words: &[&str], with: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    'scan: while i < bytes.len() {
        if matches!(bytes[i], b'{' | b'(' | b'<' | b'[') {
            for word in words {
                let end = i + 1 + word.len();
                if end < bytes.len()
                    && text.is_char_boundary(i + 1)
                    && text.is_char_boundary(end)
                    && text[i + 1..end].eq_ignore_ascii_case(word)
                    && matches!(bytes[end], b'}' | b')' | b'>' | b']')
                {
                    out.push_str(with);
                    i = end + 1;
                    continue 'scan;
                }
            }
        }
        let ch = text[i..].chars().next().unwrap_or_default();
        out.push(ch);
        i += ch.len_utf8().max(1);
    }
    out
}

/// `MaterialUtil.fromString` minus the server lookup: the Bukkit name to try, from a name, a namespaced id,
/// or a legacy numeric id (`35:14` = red wool).
pub fn material_name(input: &str) -> Option<String> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if let Some(legacy) = legacy_material(input) {
        return Some(legacy);
    }
    let name = match input.split_once(':') {
        Some((ns, rest)) if ns.eq_ignore_ascii_case("minecraft") => rest,
        _ => input,
    };
    Some(name.trim().replace(['-', ' '], "_").to_ascii_uppercase())
}

fn legacy_material(input: &str) -> Option<String> {
    let (id, data) = input.split_once(':').unwrap_or((input, "0"));
    let id: i32 = id.trim().parse().ok()?;
    let data: i8 = data.trim().parse().ok()?;
    let colour = || colour_name(data);
    let name = match id {
        35 => format!("{}_WOOL", colour()),
        95 => format!("{}_STAINED_GLASS", colour()),
        160 => format!("{}_STAINED_GLASS_PANE", colour()),
        171 => format!("{}_CARPET", colour()),
        159 => format!("{}_TERRACOTTA", colour()),
        _ => match id {
            1 => "STONE",
            2 => "GRASS_BLOCK",
            3 => "DIRT",
            4 => "COBBLESTONE",
            5 => "OAK_PLANKS",
            17 => "OAK_LOG",
            41 => "GOLD_BLOCK",
            42 => "IRON_BLOCK",
            46 => "TNT",
            47 => "BOOKSHELF",
            49 => "OBSIDIAN",
            54 => "CHEST",
            57 => "DIAMOND_BLOCK",
            58 => "CRAFTING_TABLE",
            61 => "FURNACE",
            79 => "ICE",
            80 => "SNOW_BLOCK",
            89 => "GLOWSTONE",
            133 => "EMERALD_BLOCK",
            138 => "BEACON",
            145 => "ANVIL",
            152 => "REDSTONE_BLOCK",
            173 => "COAL_BLOCK",
            264 => "DIAMOND",
            265 => "IRON_INGOT",
            266 => "GOLD_INGOT",
            276 => "DIAMOND_SWORD",
            277 => "DIAMOND_SHOVEL",
            278 => "DIAMOND_PICKAXE",
            279 => "DIAMOND_AXE",
            310 => "DIAMOND_HELMET",
            311 => "DIAMOND_CHESTPLATE",
            312 => "DIAMOND_LEGGINGS",
            313 => "DIAMOND_BOOTS",
            322 => "GOLDEN_APPLE",
            339 => "PAPER",
            340 => "BOOK",
            345 => "COMPASS",
            388 => "EMERALD",
            _ => return Some(String::new()),
        }
        .to_owned(),
    };
    Some(name)
}

fn colour_name(data: i8) -> &'static str {
    const NAMES: [&str; 16] = ["WHITE", "ORANGE", "MAGENTA", "LIGHT_BLUE", "YELLOW", "LIME", "PINK", "GRAY", "LIGHT_GRAY", "CYAN", "PURPLE", "BLUE", "BROWN", "GREEN", "RED", "BLACK"];
    NAMES.get(usize::try_from(data).unwrap_or(0)).copied().unwrap_or("WHITE")
}

/// Sets `path` (`a.b`, two levels at most) to `value` in YAML text, keeping every comment and the rest of the
/// file as it was (boostedyaml's `set` + `save`). A key that isn't there is added.
pub fn yaml_set(text: &str, path: &str, value: &str) -> String {
    let parts: Vec<&str> = path.split('.').collect();
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let indent_of = |line: &str| line.len() - line.trim_start().len();
    let key_of = |line: &str| -> Option<String> {
        let t = line.trim_start();
        if t.starts_with('#') || t.is_empty() {
            return None;
        }
        t.split_once(':').map(|(k, _)| k.trim().trim_matches(['\'', '"']).to_owned())
    };
    let write = |line: &str, value: &str| -> String {
        let indent = indent_of(line);
        let key = line.trim_start().split_once(':').map_or("", |(k, _)| k);
        format!("{}{key}: {value}", " ".repeat(indent))
    };
    match parts.as_slice() {
        [key] => {
            if let Some(i) = lines.iter().position(|l| indent_of(l) == 0 && key_of(l).as_deref() == Some(key)) {
                lines[i] = write(&lines[i], value);
            } else {
                lines.push(format!("{key}: {value}"));
            }
        }
        [parent, key] => {
            let start = lines.iter().position(|l| indent_of(l) == 0 && key_of(l).as_deref() == Some(parent));
            match start {
                Some(start) => {
                    let mut end = lines.len();
                    for (i, l) in lines.iter().enumerate().skip(start + 1) {
                        if key_of(l).is_some() && indent_of(l) == 0 {
                            end = i;
                            break;
                        }
                    }
                    let child_indent = lines[start + 1..end].iter().find(|l| key_of(l).is_some()).map_or(2, |l| indent_of(l));
                    if let Some(i) = (start + 1..end).find(|&i| indent_of(&lines[i]) == child_indent && key_of(&lines[i]).as_deref() == Some(key)) {
                        lines[i] = write(&lines[i], value);
                    } else {
                        lines.insert(start + 1, format!("{}{key}: {value}", " ".repeat(child_indent)));
                    }
                }
                None => {
                    lines.push(format!("{parent}:"));
                    lines.push(format!("  {key}: {value}"));
                }
            }
        }
        _ => {}
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// A YAML scalar for a string: single-quoted, as boostedyaml writes keys.
pub fn yaml_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids() {
        assert_eq!(dashed("069a79f444e94726a5befca90e38aaf5"), "069a79f4-44e9-4726-a5be-fca90e38aaf5");
        assert_eq!(dashed("069a79f4-44e9-4726-a5be-fca90e38aaf5"), "069a79f4-44e9-4726-a5be-fca90e38aaf5");
        assert!(looks_like_xuid(Some("2535412345678901")));
        assert!(!looks_like_xuid(Some("069a79f444e94726a5befca90e38aaf5")));
        assert!(is_blank(Some(" null ")));
    }

    #[test]
    fn responder() {
        assert_eq!(fancy(false, "Debug mode set to {0}", &["true"]), "§b[Tebex] §fDebug mode set to §6true§f");
        assert_eq!(fancy(true, "Debug mode set to {0}", &["true"]), "Debug mode set to true");
        assert_eq!(fancy(false, "Webstore URL: {0}", &["https://x.tebex.io"]), "§b[Tebex] §fWebstore URL: §6§nhttps://x.tebex.io§f");
        assert_eq!(fancy(false, "Welcome to Tebex!", &[]), "§b[Tebex] §fWelcome to §bTebex!");
        assert_eq!(error(false, "Command not found."), "§b[Tebex] §cCommand not found.");
        assert_eq!(success(false, "User {0} has been banned successfully.", &["Bob"]), "§b[Tebex] §aUser §6Bob§f has been banned successfully.");
    }

    #[test]
    fn prices() {
        assert_eq!(price(5.0), "5");
        assert_eq!(price(4.5), "4.5");
        assert_eq!(price(9.99), "9.99");
        assert_eq!(price(10.005), "10");
        assert_eq!(price(0.125), "0.12");
        assert_eq!(two_places(3.0), "3.00");
    }

    #[test]
    fn time_and_ip() {
        assert_eq!(utc_of(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_of(1_793_000_000), "2026-10-26T07:33:20Z");
        assert_eq!(anonymize_ip("192.168.1.42"), "192.168.1.x");
        assert_eq!(anonymize_ip("::1"), "::1");
    }

    #[test]
    fn placeholders() {
        assert_eq!(replace_bracketed("give {name} 1 <PLAYER> [username] (id)", &["name", "player", "username"], "Bob"), "give Bob 1 Bob Bob (id)");
        assert_eq!(replace_bracketed("lp user {uuid} (ID) {idx}", &["uuid", "id"], "U"), "lp user U U {idx}");
        assert_eq!(replace_bracketed("é {name}", &["name"], "B"), "é B");
    }

    #[test]
    fn materials() {
        assert_eq!(material_name("minecraft:diamond_sword").as_deref(), Some("DIAMOND_SWORD"));
        assert_eq!(material_name("golden apple").as_deref(), Some("GOLDEN_APPLE"));
        assert_eq!(material_name("35:14").as_deref(), Some("RED_WOOL"));
        assert_eq!(material_name("264").as_deref(), Some("DIAMOND"));
        assert_eq!(material_name("").as_deref(), None);
    }

    #[test]
    fn yaml_edits_keep_comments() {
        let text = "# top\nverbose: false\n\nserver:\n  # proxy\n  proxy: false\n  secret-key: ''\n\nconfig-version: 2\n";
        let out = yaml_set(text, "server.secret-key", &yaml_string("abc"));
        assert!(out.contains("  secret-key: 'abc'\n") && out.contains("# proxy") && out.contains("# top"));
        let out = yaml_set(&out, "verbose", "true");
        assert!(out.contains("\nverbose: true\n"));
        let parsed: serde_yaml::Value = serde_yaml::from_str(&out).unwrap();
        assert_eq!(parsed["server"]["secret-key"].as_str(), Some("abc"));
        assert_eq!(parsed["config-version"].as_i64(), Some(2));
    }
}
