//! `java.io`, `java.nio.file`, `java.time.Duration`, and the bridge between Java values and JSON.
use super::*;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// Whether a `java.nio.file.Files` call's options, passed flat or as an array, say `APPEND`.
fn appends(options: &[V]) -> bool {
    options.iter().any(|option| match option {
        V::Arr(_) | V::List(_) => option.items().unwrap_or_default().iter().any(|item| item.text() == "APPEND"),
        other => other.text() == "APPEND",
    })
}

pub struct JFile(pub PathBuf);
/// An input stream, reader or `BufferedReader`: the bytes and how far it has read.
pub struct JBytes(pub Mutex<(Vec<u8>, usize)>);
/// A writer: flushed to the file as it is closed, and at each flush.
pub struct JWriter {
    pub path: PathBuf,
    pub append: Mutex<bool>,
    pub text: Mutex<Vec<u8>>,
}

/// A file writer as Java opens one: not appending, the file is emptied (made) now, so what else writes it
/// meanwhile (`Files.copy` into a path whose stream is open) isn't wiped at the close; later flushes append.
fn opened(path: PathBuf, append: bool) -> V {
    if !append {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, b"");
    }
    native(JWriter { path, append: Mutex::new(true), text: Mutex::new(Vec::new()) })
}
pub struct JDuration(pub i64);
/// A `CharsetDecoder` or `CharsetEncoder`, by its charset's name.
pub struct JCoder(pub String);

pub fn file(path: impl Into<PathBuf>) -> V {
    native(JFile(path.into()))
}

pub fn path_of(value: &V) -> Option<PathBuf> {
    match value {
        V::Str(text) => Some(PathBuf::from(&**text)),
        other => other.native::<JFile>().map(|f| f.0.clone()),
    }
}

pub fn bytes_of(value: &V) -> Option<Vec<u8>> {
    match value {
        V::Arr(items) => Some(lock(items).iter().map(|b| b.int() as u8).collect()),
        V::Str(text) => Some(text.as_bytes().to_vec()),
        other => other.native::<JBytes>().map(|b| {
            let state = lock(&b.0);
            state.0[state.1..].to_vec()
        }),
    }
}

pub fn to_byte_array(bytes: &[u8]) -> V {
    arr(bytes.iter().map(|b| V::Int(i32::from(*b as i8))).collect())
}

pub fn stream(bytes: Vec<u8>) -> V {
    native(JBytes(Mutex::new((bytes, 0))))
}

fn io_error(error: std::io::Error) -> V {
    let class = if error.kind() == std::io::ErrorKind::NotFound { "FileNotFoundException" } else { "IOException" };
    exc(class, error.to_string())
}

fn read_file(path: &Path) -> Result<Vec<u8>, V> {
    std::fs::read(path).map_err(io_error)
}

fn duration(ms: i64) -> V {
    native(JDuration(ms))
}

/// A `ChronoUnit` or `TimeUnit` (its name here) in milliseconds.
fn unit_millis(unit: &str) -> i64 {
    match unit {
        "SECONDS" => 1000,
        "MINUTES" => 60_000,
        "HOURS" => 3_600_000,
        "HALF_DAYS" => 43_200_000,
        "DAYS" => 86_400_000,
        "WEEKS" => 604_800_000,
        _ => 1,
    }
}

/// `Duration.parse`: "PT20.345S", "P2D", "PT15M", "-PT6H3M".
fn parse_duration(text: &str) -> Option<i64> {
    let upper = text.trim().to_ascii_uppercase();
    let (negative, rest) = upper.strip_prefix('-').map_or((false, upper.as_str()), |r| (true, r));
    let rest = rest.strip_prefix('+').unwrap_or(rest).strip_prefix('P')?;
    let (days, time) = rest.split_once('T').unwrap_or((rest, ""));
    let mut ms = 0f64;
    if !days.is_empty() {
        ms += days.strip_suffix('D')?.parse::<f64>().ok()? * 86_400_000.0;
    }
    let mut number = String::new();
    for c in time.chars() {
        match c {
            'H' | 'M' | 'S' => {
                let value: f64 = number.parse().ok()?;
                ms += value * match c { 'H' => 3_600_000.0, 'M' => 60_000.0, _ => 1000.0 };
                number.clear();
            }
            _ => number.push(c),
        }
    }
    if !number.is_empty() || (days.is_empty() && time.is_empty()) {
        return None;
    }
    Some(if negative { -ms as i64 } else { ms as i64 })
}

/// `Duration.toString`: "PT0S", "PT1H2M3.5S", "PT-0.5S".
pub fn duration_text(ms: i64) -> String {
    if ms == 0 {
        return "PT0S".to_owned();
    }
    let (hours, minutes) = (ms / 3_600_000, ms % 3_600_000 / 60_000);
    let rest = ms % 60_000;
    let mut out = String::from("PT");
    if hours != 0 {
        out.push_str(&format!("{hours}H"));
    }
    if minutes != 0 {
        out.push_str(&format!("{minutes}M"));
    }
    if rest != 0 {
        let (secs, millis) = (rest / 1000, (rest % 1000).abs());
        let sign = if rest < 0 && secs == 0 { "-" } else { "" };
        if millis == 0 {
            out.push_str(&format!("{sign}{secs}S"));
        } else {
            let frac = format!("{millis:03}");
            out.push_str(&format!("{sign}{secs}.{}S", frac.trim_end_matches('0')));
        }
    }
    out
}

#[cfg(test)]
mod duration_tests {
    #[test]
    fn duration_text_reads_like_java() {
        assert_eq!(super::duration_text(0), "PT0S");
        assert_eq!(super::duration_text(3_723_500), "PT1H2M3.5S");
        assert_eq!(super::duration_text(60_000), "PT1M");
        assert_eq!(super::duration_text(-500), "PT-0.5S");
        assert_eq!(super::duration_text(90_061_000), "PT25H1M1S");
        assert_eq!(super::parse_duration("PT20.345S"), Some(20_345));
        assert_eq!(super::parse_duration("P2D"), Some(172_800_000));
        assert_eq!(super::parse_duration("-PT6H3M"), Some(-21_780_000));
        assert_eq!(super::parse_duration("PT"), None);
    }
}

fn lines_of(bytes: &[u8]) -> Vec<V> {
    String::from_utf8_lossy(bytes).lines().map(s).collect()
}

pub fn field(class: &str, name: &str) -> Option<V> {
    Some(match (class, name) {
        ("StandardCharsets", _) | ("StandardOpenOption", _) => s(name),
        ("Duration", "ZERO") => duration(0),
        ("File", "separator") => s(std::path::MAIN_SEPARATOR_STR),
        _ => return None,
    })
}

/// `java.io.RandomAccessFile`: the file's bytes and a pointer; what is written goes to the file on `close`.
/// (CoreProtect reads its `blacklist.txt` with one while it starts: without it, a server that had the file
/// couldn't enable CoreProtect.)
pub struct JRaf {
    path: PathBuf,
    state: Mutex<(Vec<u8>, usize, bool)>,
}

fn raf_open(path: PathBuf, mode: &str) -> R {
    let data = match std::fs::read(&path) {
        Ok(data) => data,
        Err(_) if mode.contains('w') => {
            std::fs::write(&path, b"").map_err(|e| exc("FileNotFoundException", format!("{} ({e})", path.display())))?;
            Vec::new()
        }
        Err(e) => return Err(exc("FileNotFoundException", format!("{} ({e})", path.display()))),
    };
    Ok(native(JRaf { path, state: Mutex::new((data, 0, false)) }))
}

fn raf_method(f: &JRaf, name: &str, a: &[V]) -> Option<R> {
    let mut state = lock(&f.state);
    let (data, at, dirty) = &mut *state;
    let put = |bytes: &[u8], data: &mut Vec<u8>, at: &mut usize| {
        if data.len() < *at + bytes.len() {
            data.resize(*at + bytes.len(), 0);
        }
        data[*at..*at + bytes.len()].copy_from_slice(bytes);
        *at += bytes.len();
    };
    Some(Ok(match name {
        "length" => V::Long(data.len() as i64),
        "getFilePointer" => V::Long(*at as i64),
        "seek" => {
            *at = a[0].long().max(0) as usize;
            V::Null
        }
        "skipBytes" => {
            let n = a[0].long().max(0) as usize;
            let skipped = n.min(data.len().saturating_sub(*at));
            *at += skipped;
            V::Int(skipped as i32)
        }
        // Java's readLine: bytes as Latin-1 up to `\n`, `\r` dropped; null at the end.
        "readLine" => {
            if *at >= data.len() {
                return Some(Ok(V::Null));
            }
            let mut line = String::new();
            while *at < data.len() {
                let byte = data[*at];
                *at += 1;
                match byte {
                    b'\n' => break,
                    b'\r' => {
                        if data.get(*at) == Some(&b'\n') {
                            *at += 1;
                        }
                        break;
                    }
                    other => line.push(other as char),
                }
            }
            s(&line)
        }
        "read" if a.is_empty() => match data.get(*at) {
            Some(byte) => {
                *at += 1;
                V::Int(i32::from(*byte))
            }
            None => V::Int(-1),
        },
        "read" | "readFully" => {
            let V::Arr(items) = &a[0] else { return None };
            let mut items = lock(items);
            let (off, len) = if a.len() == 3 { (a[1].int().max(0) as usize, a[2].int().max(0) as usize) } else { (0, items.len()) };
            let n = len.min(data.len().saturating_sub(*at));
            if n == 0 && len > 0 {
                return Some(if name == "readFully" { Err(exc("EOFException", "")) } else { Ok(V::Int(-1)) });
            }
            for i in 0..n {
                items[off + i] = V::Int(i32::from(data[*at + i] as i8));
            }
            *at += n;
            V::Int(n as i32)
        }
        "write" => {
            match &a[0] {
                V::Arr(_) => {
                    let all = bytes_of(&a[0]).unwrap_or_default();
                    let slice = if a.len() == 3 { all.get(a[1].int().max(0) as usize..(a[1].int() + a[2].int()).max(0) as usize).unwrap_or_default().to_vec() } else { all };
                    put(&slice, data, at);
                }
                other => put(&[other.int() as u8], data, at),
            }
            *dirty = true;
            V::Null
        }
        "writeBytes" => {
            let bytes: Vec<u8> = a[0].text().chars().map(|c| c as u8).collect();
            put(&bytes, data, at);
            *dirty = true;
            V::Null
        }
        "setLength" => {
            data.resize(a[0].long().max(0) as usize, 0);
            *at = (*at).min(data.len());
            *dirty = true;
            V::Null
        }
        "close" => {
            if std::mem::take(dirty) {
                if let Err(e) = std::fs::write(&f.path, &*data) {
                    return Some(Err(exc("IOException", e.to_string())));
                }
            }
            V::Null
        }
        _ => return None,
    }))
}

pub fn construct(class: &str, a: &[V]) -> Option<R> {
    let path = || a.first().and_then(path_of).ok_or_else(|| exc("NullPointerException", "no path"));
    if class == "RandomAccessFile" && a.len() == 2 {
        return Some(path().and_then(|p| raf_open(p, &a[1].text())));
    }
    Some(match (class, a.len()) {
        ("File", 2) => match path_of(&a[0]) {
            Some(parent) => Ok(file(parent.join(a[1].text()))),
            None => Ok(file(a[1].text())),
        },
        ("File", 1) => path().map(file),
        ("FileReader" | "FileInputStream", _) => path().and_then(|p| read_file(&p)).map(stream),
        ("InputStreamReader" | "BufferedReader" | "BufferedInputStream" | "PushbackInputStream" | "PushbackReader", _) => Ok(a[0].clone()),
        ("ByteArrayInputStream", 1) => Ok(stream(bytes_of(&a[0]).unwrap_or_default())),
        ("FileWriter" | "FileOutputStream", _) => path().map(|p| opened(p, a.get(1).is_some_and(V::truth))),
        ("OutputStreamWriter" | "BufferedWriter" | "BufferedOutputStream" | "FastBufferedOutputStream", _) => Ok(a[0].clone()),
        ("FastBufferedInputStream", _) => Ok(a[0].clone()),
        ("PrintWriter" | "PrintStream", _) if a[0].native::<JWriter>().is_some() => Ok(a[0].clone()),
        // Over a stream or writer already made (`new PrintWriter(new ByteArrayOutputStream())`, a plugin's own
        // OutputStream): writes go to it. A file name or a File opens the file.
        ("PrintWriter" | "PrintStream", _) if !matches!(a[0], V::Str(_)) && a[0].native::<JFile>().is_none() => Ok(a[0].clone()),
        ("PrintWriter" | "PrintStream", _) => path().map(|p| opened(p, false)),
        ("String", 0) => Ok(s("")),
        // `new String(chars | codePoints | bytes [, offset, count] [, charset])`. With an offset and count, that piece only
        // (snakeyaml-engine's reader cuts every token out of its window so; the whole window came back). A `byte[]` holds
        // Java's signed bytes, so a negative item means UTF-8 bytes; an `int[]` of code points never has one.
        ("String", _) => Ok(match &a[0] {
            V::Arr(items) => {
                let items = lock(items);
                let (from, to) = match a {
                    [_, V::Int(off), V::Int(count), ..] => {
                        let from = ((*off).max(0) as usize).min(items.len());
                        (from, (from + (*count).max(0) as usize).min(items.len()))
                    }
                    _ => (0, items.len()),
                };
                let piece = &items[from..to];
                if items.first().is_some_and(|c| matches!(c, V::Char(_))) {
                    s(&piece.iter().map(V::text).collect::<String>())
                } else if piece.iter().any(|b| b.int() < 0) || a.len() == 2 || a.len() == 4 {
                    let bytes: Vec<u8> = piece.iter().map(|b| b.int() as u8).collect();
                    s(&String::from_utf8_lossy(&bytes))
                } else {
                    s(&piece.iter().map(|c| char::from_u32(c.int() as u32).unwrap_or(char::REPLACEMENT_CHARACTER)).collect::<String>())
                }
            }
            other => s(&other.text()),
        }),
        _ => return None,
    })
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if let Some(m) = this.native::<JAttrs>() {
        return attrs_method(m, name);
    }
    if let Some(f) = this.native::<JRaf>() {
        return raf_method(f, name, a);
    }
    if let Some(t) = this.native::<JFileTime>() {
        return file_time_method(t, name, a);
    }
    if let V::Str(text) = this {
        // A `Charset` travels as its name (`StandardCharsets.UTF_8` is "UTF_8"): its decoder and encoder.
        if matches!(name, "newDecoder" | "newEncoder") && a.is_empty() {
            return Some(Ok(native(JCoder(text.to_string()))));
        }
        return (name == "getBytes").then(|| Ok(to_byte_array(text.as_bytes())));
    }
    if let Some(coder) = this.native::<JCoder>() {
        return Some(Ok(match name {
            "onMalformedInput" | "onUnmappableCharacter" | "replaceWith" | "reset" => this.clone(),
            "charset" => s(&coder.0),
            // Bytes to text, and text to bytes: what a `CharsetDecoder`/`CharsetEncoder` does (UTF-8 here).
            "decode" => match a.first().and_then(bytes_of) {
                Some(bytes) => match String::from_utf8(bytes) {
                    Ok(text) => s(&text),
                    Err(_) => return Some(Err(exc("MalformedInputException", "Input length = 1"))),
                },
                None => return Some(Err(exc("NullPointerException", "decode of null"))),
            },
            "encode" => to_byte_array(a.first().map(V::text).unwrap_or_default().as_bytes()),
            "averageCharsPerByte" | "maxCharsPerByte" | "averageBytesPerChar" => V::Double(1.0),
            "maxBytesPerChar" => V::Double(3.0),
            "canEncode" => V::Bool(true),
            "toString" => s(&coder.0),
            _ => return None,
        }));
    }
    if let Some(f) = this.native::<JFile>() {
        return file_method(&f.0, this, name, a);
    }
    if let Some(input) = this.native::<JBytes>() {
        return input_method(input, name, a);
    }
    if let Some(writer) = this.native::<JWriter>() {
        // `write(buffer, offset, length)`: that part of the array or text only.
        if name == "write" && a.len() == 3 && matches!(a[1], V::Int(_) | V::Long(_)) {
            let (from, count) = (a[1].int().max(0) as usize, a[2].int().max(0) as usize);
            let part: Vec<u8> = match &a[0] {
                V::Arr(items) if lock(items).first().is_some_and(|c| matches!(c, V::Char(_))) => lock(items).iter().skip(from).take(count).map(V::text).collect::<String>().into_bytes(),
                V::Arr(_) => bytes_of(&a[0]).unwrap_or_default().into_iter().skip(from).take(count).collect(),
                other => other.text().chars().skip(from).take(count).collect::<String>().into_bytes(),
            };
            lock(&writer.text).extend_from_slice(&part);
            return Some(Ok(V::Null));
        }
        // `Writer.append(x)` returns the writer: `w.append(a).append(b)`.
        if name == "append" {
            return Some(writer_method(writer, name, a).unwrap_or(Ok(V::Null)).map(|_| this.clone()));
        }
        return writer_method(writer, name, a);
    }
    if let Some(d) = this.native::<JDuration>() {
        let ms = d.0;
        return Some(Ok(match name {
            "toMillis" => V::Long(ms),
            "getSeconds" | "toSeconds" => V::Long(ms / 1000),
            "toMinutes" => V::Long(ms / 60_000),
            "toHours" => V::Long(ms / 3_600_000),
            "toDays" => V::Long(ms / 86_400_000),
            "toNanos" => V::Long(ms * 1_000_000),
            "isZero" => V::Bool(ms == 0),
            "isNegative" => V::Bool(ms < 0),
            "plus" => duration(ms + a[0].native::<JDuration>().map_or(0, |o| o.0)),
            "minus" => duration(ms - a[0].native::<JDuration>().map_or(0, |o| o.0)),
            "compareTo" => V::Int(ms.cmp(&a[0].native::<JDuration>().map_or(0, |o| o.0)) as i32),
            "multipliedBy" => duration(ms * a[0].long()),
            "dividedBy" if a[0].native::<JDuration>().is_some() => V::Long(ms / a[0].native::<JDuration>().map_or(1, |o| o.0).max(1)),
            "dividedBy" => duration(ms / a[0].long().max(1)),
            "negated" => duration(-ms),
            "abs" => duration(ms.abs()),
            "isPositive" => V::Bool(ms > 0),
            "toDaysPart" => V::Long(ms / 86_400_000),
            "toHoursPart" => V::Int((ms / 3_600_000 % 24) as i32),
            "toMinutesPart" => V::Int((ms / 60_000 % 60) as i32),
            "toSecondsPart" => V::Int((ms / 1000 % 60) as i32),
            "toMillisPart" => V::Int((ms % 1000) as i32),
            "toNanosPart" | "getNano" => V::Int((ms % 1000 * 1_000_000) as i32),
            "plusMillis" => duration(ms + a[0].long()),
            "plusSeconds" => duration(ms + a[0].long() * 1000),
            "plusMinutes" => duration(ms + a[0].long() * 60_000),
            "plusHours" => duration(ms + a[0].long() * 3_600_000),
            "plusDays" => duration(ms + a[0].long() * 86_400_000),
            "minusMillis" => duration(ms - a[0].long()),
            "minusSeconds" => duration(ms - a[0].long() * 1000),
            "minusMinutes" => duration(ms - a[0].long() * 60_000),
            "minusHours" => duration(ms - a[0].long() * 3_600_000),
            "minusDays" => duration(ms - a[0].long() * 86_400_000),
            "equals" => V::Bool(a[0].native::<JDuration>().is_some_and(|o| o.0 == ms)),
            "hashCode" => V::Int((ms ^ (ms >> 32)) as i32),
            "toString" => s(&duration_text(ms)),
            _ => return None,
        }));
    }
    None
}

/// A `FilenameFilter` (`accept(dir, name)`) or a `FileFilter` (`accept(file)`). A lambda doesn't say which:
/// it is asked as a FileFilter first, and as a FilenameFilter when that throws (its `name` was missing).
fn file_filter(filter: &V, dir: &V, entry: &Path) -> Result<bool, V> {
    let name = s(&entry.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    let ask = |args: &[V]| match filter {
        V::Func(_) => crate::call(filter, "$invoke", args),
        _ => crate::call(filter, "accept", args),
    };
    if matches!(filter, V::Obj(_)) {
        return ask(&if crate::instance_of(filter, "FilenameFilter") { vec![dir.clone(), name] } else { vec![file(entry.to_owned())] }).map(|v| v.truth());
    }
    match ask(&[file(entry.to_owned())]) {
        Ok(v) => Ok(v.truth()),
        Err(_) => ask(&[dir.clone(), name]).map(|v| v.truth()),
    }
}

fn file_method(path: &Path, this: &V, name: &str, a: &[V]) -> Option<R> {
    let out = |result: std::io::Result<()>| result.map(|()| V::Bool(true)).map_err(io_error);
    Some(match name {
        "exists" => Ok(V::Bool(path.exists())),
        // `Path.startsWith(other)` / `endsWith(other)`: by whole parts, as Java's are.
        "startsWith" | "endsWith" if a.len() == 1 => {
            let other = path_of(&a[0]).unwrap_or_default();
            Ok(V::Bool(if name == "startsWith" { path.starts_with(&other) } else { path.ends_with(&other) }))
        }
        "isDirectory" => Ok(V::Bool(path.is_dir())),
        "isFile" => Ok(V::Bool(path.is_file())),
        "canRead" | "canWrite" | "canExecute" => Ok(V::Bool(path.exists())),
        "getName" => Ok(s(&path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())),
        "getFileName" => Ok(file(path.file_name().map(PathBuf::from).unwrap_or_default())),
        "getPath" | "toString" => Ok(s(&path.to_string_lossy())),
        "getAbsolutePath" | "getCanonicalPath" => Ok(s(&std::path::absolute(path).unwrap_or_else(|_| path.to_owned()).to_string_lossy())),
        "getAbsoluteFile" | "toAbsolutePath" | "getCanonicalFile" | "normalize" => Ok(file(std::path::absolute(path).unwrap_or_else(|_| path.to_owned()))),
        "getParent" if a.is_empty() && false => Ok(V::Null),
        "getParentFile" | "getParent" => Ok(path.parent().map_or(V::Null, file)),
        "toPath" | "toFile" => Ok(this.clone()),
        "relativize" => Ok(file(path_of(&a[0]).and_then(|other| other.strip_prefix(path).ok().map(PathBuf::from)).unwrap_or_default())),
        "resolveSibling" => Ok(file(path.with_file_name(a[0].text()))),
        "getNameCount" => Ok(V::Int(path.components().count() as i32)),
        "isAbsolute" => Ok(V::Bool(path.is_absolute())),
        "resolve" => Ok(file(path.join(a[0].text()))),
        "mkdir" => Ok(V::Bool(std::fs::create_dir(path).is_ok())),
        "mkdirs" => Ok(V::Bool(std::fs::create_dir_all(path).is_ok())),
        "createNewFile" => out(std::fs::OpenOptions::new().write(true).create_new(true).open(path).map(|_| ())),
        "delete" => Ok(V::Bool(if path.is_dir() { std::fs::remove_dir(path).is_ok() } else { std::fs::remove_file(path).is_ok() })),
        // `tmp.renameTo(file)`: true when it moved (a file already there is replaced, as on Windows' Java it isn't:
        // Java fails then, so do we).
        "renameTo" if a.len() == 1 => Ok(V::Bool(path_of(&a[0]).is_some_and(|to| !(to.exists() && cfg!(windows)) && std::fs::rename(path, to).is_ok()))),
        "deleteOnExit" => Ok(V::Null),
        "setLastModified" | "setReadable" | "setWritable" | "setExecutable" | "setReadOnly" => Ok(V::Bool(path.exists())),
        "getUsableSpace" | "getFreeSpace" | "getTotalSpace" => Ok(V::Long(1 << 40)),
        "toURI" | "toUri" => Ok(s(&format!("file:///{}", std::path::absolute(path).unwrap_or_else(|_| path.to_owned()).to_string_lossy().replace('\\', "/").trim_start_matches('/')))),
        "length" => Ok(V::Long(std::fs::metadata(path).map_or(0, |m| m.len() as i64))),
        "lastModified" => Ok(V::Long(std::fs::metadata(path).ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as i64))),
        "listFiles" | "list" => Ok(match std::fs::read_dir(path) {
            Ok(entries) => {
                let mut kept = Vec::new();
                for entry in entries.flatten() {
                    // `listFiles((dir, name) -> name.endsWith(".jar"))` or `listFiles(File::isDirectory)`: only what the filter takes.
                    if let Some(filter) = a.first().filter(|f| !f.is_null()) {
                        match file_filter(filter, this, &entry.path()) {
                            Ok(true) => {}
                            Ok(false) => continue,
                            Err(e) => return Some(Err(e)),
                        }
                    }
                    kept.push(if name == "list" { s(&entry.file_name().to_string_lossy()) } else { file(entry.path()) });
                }
                arr(kept)
            }
            Err(_) => V::Null,
        }),
        "hashCode" => Ok(V::Int(path.to_string_lossy().bytes().fold(0, |h: i32, b| h.wrapping_mul(31).wrapping_add(i32::from(b))))),
        _ => return None,
    })
}

fn input_method(input: &JBytes, name: &str, a: &[V]) -> Option<R> {
    let mut state = lock(&input.0);
    Some(Ok(match name {
        "close" | "mark" | "reset" => V::Null,
        "ready" => V::Bool(state.1 < state.0.len()),
        // `read(buffer[, offset, length])`: a `char[]` takes text (a reader), any other array bytes (a stream).
        // The count read, or -1 at the end, as Java says.
        "read" if matches!(a.first(), Some(V::Arr(_))) => {
            let V::Arr(buffer) = &a[0] else { return None };
            let mut buffer = lock(buffer);
            let offset = a.get(1).map_or(0, |n| n.int().max(0) as usize).min(buffer.len());
            let length = a.get(2).map_or(buffer.len() - offset, |n| n.int().max(0) as usize).min(buffer.len() - offset);
            if length == 0 {
                return Some(Ok(V::Int(0)));
            }
            if state.1 >= state.0.len() {
                return Some(Ok(V::Int(-1)));
            }
            let chars = buffer.first().is_some_and(|c| matches!(c, V::Char(_)));
            let mut count = 0;
            if chars {
                // Whole UTF-8 characters, one `char` each (a `V::Char` holds any character, beyond the BMP too).
                while count < length && state.1 < state.0.len() {
                    let rest = &state.0[state.1..];
                    let width = match rest[0] { b if b < 0x80 => 1, b if b >= 0xF0 => 4, b if b >= 0xE0 => 3, b if b >= 0xC0 => 2, _ => 1 }.min(rest.len());
                    let ch = std::str::from_utf8(&rest[..width]).ok().and_then(|t| t.chars().next()).unwrap_or('\u{FFFD}');
                    buffer[offset + count] = V::Char(ch);
                    count += 1;
                    state.1 += width;
                }
            } else {
                while count < length && state.1 < state.0.len() {
                    buffer[offset + count] = V::Int(i32::from(state.0[state.1] as i8));
                    count += 1;
                    state.1 += 1;
                }
            }
            V::Int(count as i32)
        }
        "skip" => {
            let n = a.first().map_or(0, |n| n.long().max(0) as usize).min(state.0.len() - state.1.min(state.0.len()));
            state.1 += n;
            V::Long(n as i64)
        }
        "available" => V::Int(state.0.len().saturating_sub(state.1) as i32),
        // A pushback stream's `unread(bytes, offset, length)` or `unread(byte)`: what was read goes back.
        "unread" => {
            let n = match a {
                [V::Arr(_), _, length] => length.int().max(0) as usize,
                [V::Arr(items)] => lock(items).len(),
                _ => 1,
            };
            state.1 = state.1.saturating_sub(n);
            V::Null
        }
        "markSupported" => V::Bool(false),
        "read" => match state.0.get(state.1).copied() {
            Some(byte) => {
                state.1 += 1;
                V::Int(i32::from(byte))
            }
            None => V::Int(-1),
        },
        "readAllBytes" => {
            let at = state.1;
            state.1 = state.0.len();
            to_byte_array(&state.0[at..])
        }
        // `in.transferTo(out)`: the rest of the stream, written to `out`; how many bytes.
        "transferTo" if a.len() == 1 => {
            let at = state.1;
            state.1 = state.0.len();
            let rest = state.0[at..].to_vec();
            drop(state);
            let count = rest.len() as i64;
            return Some(crate::call(&a[0], "write", &[to_byte_array(&rest)]).map(|_| V::Long(count)));
        }
        "readLine" => {
            let at = state.1;
            if at >= state.0.len() {
                return Some(Ok(V::Null));
            }
            let rest = &state.0[at..];
            let end = rest.iter().position(|b| *b == b'\n').unwrap_or(rest.len());
            let line = String::from_utf8_lossy(&rest[..end]).trim_end_matches('\r').to_owned();
            state.1 = at + end + 1;
            s(&line)
        }
        "lines" => {
            let at = state.1;
            V::Stream(Arc::new(Mutex::new(lines_of(&state.0[at..]))))
        }
        _ => return None,
    }))
}

fn writer_method(writer: &JWriter, name: &str, a: &[V]) -> Option<R> {
    let add = |value: &[u8]| lock(&writer.text).extend_from_slice(value);
    let piece = |value: &V| bytes_of(value).filter(|_| !matches!(value, V::Str(_))).unwrap_or_else(|| value.text().into_bytes());
    Some(match name {
        "write" | "print" | "append" => {
            add(&piece(&a[0]));
            Ok(V::Null)
        }
        "println" => {
            if let Some(line) = a.first() {
                add(&piece(line));
            }
            add(b"\n");
            Ok(V::Null)
        }
        "newLine" => {
            add(b"\n");
            Ok(V::Null)
        }
        "flush" | "close" => {
            let text = std::mem::take(&mut *lock(&writer.text));
            let append = std::mem::replace(&mut *lock(&writer.append), true);
            if let Some(parent) = writer.path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let written = if append {
                use std::io::Write;
                std::fs::OpenOptions::new().append(true).create(true).open(&writer.path).and_then(|mut f| f.write_all(&text))
            } else {
                std::fs::write(&writer.path, &text)
            };
            written.map(|()| V::Null).map_err(io_error)
        }
        _ => return None,
    })
}

/// `java.nio.file.attribute.FileTime`: a time in milliseconds since 1970.
pub struct JFileTime(pub i64);

fn file_time_method(t: &JFileTime, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match name {
        "toMillis" => V::Long(t.0),
        "toInstant" => crate::call_static("Instant", "ofEpochMilli", &[V::Long(t.0)]).unwrap_or(V::Long(t.0)),
        "compareTo" => V::Int(t.0.cmp(&a[0].native::<JFileTime>().map_or(a[0].long(), |o| o.0)) as i32),
        "equals" => V::Bool(a[0].native::<JFileTime>().is_some_and(|o| o.0 == t.0)),
        "hashCode" => V::Int(t.0 as i32),
        "toString" => crate::call_static("Instant", "ofEpochMilli", &[V::Long(t.0)]).map_or_else(|_| s(&t.0.to_string()), |i| s(&i.text())),
        _ => return None,
    }))
}

/// A file's `BasicFileAttributes`.
pub struct JAttrs(std::fs::Metadata);

fn attrs_method(m: &JAttrs, name: &str) -> Option<R> {
    let millis = |t: std::io::Result<std::time::SystemTime>| native(JFileTime(t.ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as i64)));
    Some(Ok(match name {
        "size" => V::Long(m.0.len() as i64),
        "isDirectory" => V::Bool(m.0.is_dir()),
        "isRegularFile" => V::Bool(m.0.is_file()),
        "isSymbolicLink" => V::Bool(m.0.file_type().is_symlink()),
        "isOther" => V::Bool(false),
        "lastModifiedTime" => millis(m.0.modified()),
        "lastAccessTime" => millis(m.0.accessed()),
        "creationTime" => millis(m.0.created()),
        "fileKey" => V::Null,
        _ => return None,
    }))
}

/// What a FileVisitor's answer says: "CONTINUE", "SKIP_SUBTREE", "SKIP_SIBLINGS" or "TERMINATE".
fn visit(visitor: &V, name: &str, path: &std::path::Path, second: V) -> Result<String, V> {
    if let V::Obj(object) = visitor {
        if crate::method_of(object.class, name, &[V::Null, V::Null]).is_none() {
            return Ok("CONTINUE".into());
        }
    }
    let answer = crate::call(visitor, name, &[file(path.to_path_buf()), second])?;
    Ok(if answer.is_null() { "CONTINUE".into() } else { answer.text() })
}

fn walk_tree(at: &std::path::Path, visitor: &V, depth: usize) -> Result<bool, V> {
    let meta = match std::fs::metadata(at) {
        Ok(meta) => meta,
        Err(e) => return visit(visitor, "visitFileFailed", at, exc("IOException", e.to_string())).map(|a| a != "TERMINATE"),
    };
    if !meta.is_dir() || depth == 0 {
        return visit(visitor, "visitFile", at, native(JAttrs(meta))).map(|a| a != "TERMINATE");
    }
    match visit(visitor, "preVisitDirectory", at, native(JAttrs(meta)))?.as_str() {
        "TERMINATE" => return Ok(false),
        "SKIP_SUBTREE" | "SKIP_SIBLINGS" => return Ok(true),
        _ => {}
    }
    let mut entries: Vec<_> = std::fs::read_dir(at).map(|d| d.filter_map(Result::ok).map(|e| e.path()).collect()).unwrap_or_default();
    entries.sort();
    for entry in entries {
        if !walk_tree(&entry, visitor, depth - 1)? {
            return Ok(false);
        }
    }
    visit(visitor, "postVisitDirectory", at, V::Null).map(|a| a != "TERMINATE")
}

pub fn static_call(class: &str, name: &str, a: &[V]) -> Option<R> {
    // `Files.getLastModifiedTime(path)` / `FileTime.fromMillis(ms)` (CoreProtect compares its language files' times).
    match (class, name) {
        ("Files", "getLastModifiedTime") => {
            let path = a.first().and_then(path_of).unwrap_or_default();
            return Some(match std::fs::metadata(&path).and_then(|m| m.modified()) {
                Ok(time) => Ok(native(JFileTime(time.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)))),
                Err(e) => Err(exc("NoSuchFileException", format!("{} ({e})", path.display()))),
            });
        }
        ("FileTime", "fromMillis") => return Some(Ok(native(JFileTime(a[0].long())))),
        // `Files.walkFileTree(start, visitor)` / `(start, options, depth, visitor)`: each file to `visitFile(path,
        // attrs)`, each folder to `preVisitDirectory` before and `postVisitDirectory` after, as far as the visitor
        // overrides them; TERMINATE stops, SKIP_SUBTREE skips a folder (FAWE sums its history's size this way).
        ("Files", "walkFileTree") if !a.is_empty() => {
            let start = path_of(&a[0]).unwrap_or_default();
            let visitor = a.last().cloned().unwrap_or(V::Null);
            let depth = if a.len() == 4 { a[2].int().max(0) as usize } else { usize::MAX };
            return Some(walk_tree(&start, &visitor, depth).map(|_| a[0].clone()));
        }
        ("Files", "readAttributes") if !a.is_empty() => {
            let path = path_of(&a[0]).unwrap_or_default();
            return Some(match std::fs::metadata(&path) {
                Ok(meta) => Ok(native(JAttrs(meta))),
                Err(e) => Err(exc("NoSuchFileException", format!("{} ({e})", path.display()))),
            });
        }
        _ => {}
    }
    // A `Charset` is its name here (`StandardCharsets.UTF_8` is "UTF_8"); every name is supported.
    if class == "Charset" {
        return match (name, a.len()) {
            ("forName", 1 | 2) => Some(Ok(a[0].clone())),
            ("defaultCharset", 0) => Some(Ok(s("UTF-8"))),
            ("isSupported", 1) => Some(Ok(V::Bool(true))),
            _ => None,
        };
    }
    if class == "Duration" {
        let n = a.first().map_or(0, V::long);
        return Some(Ok(duration(match name {
            "ofMillis" => n,
            // `ofSeconds(seconds, nanoAdjustment)`.
            "ofSeconds" => n * 1000 + a.get(1).map_or(0, V::long) / 1_000_000,
            "ofMinutes" => n * 60_000,
            "of" => n * a.get(1).map_or(1, |unit| unit_millis(&unit.text())),
            "parse" => return Some(parse_duration(&a[0].text()).map(duration).ok_or_else(|| exc("DateTimeParseException", format!("Text cannot be parsed to a Duration: {}", a[0].text())))),
            "ofHours" => n * 3_600_000,
            "ofDays" => n * 86_400_000,
            "ofNanos" => n / 1_000_000,
            _ => return None,
        })));
    }
    if matches!((class, name), ("Paths", "get") | ("Path", "of")) {
        let mut path = PathBuf::from(a[0].text());
        if let Some(rest) = a.get(1) {
            for part in rest.items().unwrap_or_default() {
                path.push(part.text());
            }
        }
        return Some(Ok(file(path)));
    }
    // java.io's `File.createTempFile(prefix, suffix[, directory])` (the directory last, unlike Files'): Tebex copies its
    // bundled config through one.
    if class == "File" && name == "createTempFile" {
        let prefix = a.first().map(V::text).unwrap_or_default();
        if prefix.chars().count() < 3 {
            return Some(Err(exc("IllegalArgumentException", "Prefix string too short")));
        }
        let suffix = a.get(1).filter(|v| !v.is_null()).map_or(".tmp".to_owned(), V::text);
        let dir = match a.get(2).filter(|v| !v.is_null()) {
            Some(directory) => path_of(directory)?,
            None => std::env::temp_dir(),
        };
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let target = dir.join(format!("{prefix}{stamp}{suffix}"));
        let _ = std::fs::create_dir_all(&dir);
        return Some(std::fs::write(&target, b"").map(|()| file(target)).map_err(io_error));
    }
    if class != "Files" {
        return None;
    }
    // `createTempFile([dir,] prefix, suffix)` and `createTempDirectory([dir,] prefix)`: a new name each time.
    if matches!(name, "createTempFile" | "createTempDirectory") {
        let in_dir = a.len() >= 3 || (name == "createTempDirectory" && a.len() >= 2 && a[0].native::<JFile>().is_some());
        let dir = if in_dir { path_of(&a[0])? } else { std::env::temp_dir() };
        let rest = if in_dir { &a[1..] } else { a };
        let prefix = rest.first().filter(|v| !v.is_null()).map_or(String::new(), V::text);
        let suffix = if name == "createTempFile" { rest.get(1).filter(|v| !v.is_null()).map_or(".tmp".to_owned(), V::text) } else { String::new() };
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let target = dir.join(format!("{prefix}{stamp}{suffix}"));
        let _ = std::fs::create_dir_all(&dir);
        let made = if name == "createTempFile" { std::fs::write(&target, b"") } else { std::fs::create_dir_all(&target) };
        return Some(made.map(|()| file(target)).map_err(io_error));
    }
    if name == "copy" && a[0].native::<JBytes>().is_some() {
        let target = path_of(&a[1])?;
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        return Some(std::fs::write(&target, bytes_of(&a[0]).unwrap_or_default()).map(|()| V::Long(0)).map_err(io_error));
    }
    let path = path_of(a.first()?)?;
    let done = |result: std::io::Result<()>, value: V| result.map(|()| value).map_err(io_error);
    Some(match name {
        "exists" => Ok(V::Bool(path.exists())),
        "notExists" => Ok(V::Bool(!path.exists())),
        "isDirectory" => Ok(V::Bool(path.is_dir())),
        "isRegularFile" => Ok(V::Bool(path.is_file())),
        "isReadable" | "isWritable" => Ok(V::Bool(path.exists())),
        "isSymbolicLink" => Ok(V::Bool(path.is_symlink())),
        "size" => std::fs::metadata(&path).map(|m| V::Long(m.len() as i64)).map_err(io_error),
        "createDirectories" | "createDirectory" => done(std::fs::create_dir_all(&path), a[0].clone()),
        "createFile" => done(std::fs::write(&path, ""), a[0].clone()),
        "delete" => done(std::fs::remove_file(&path), V::Null),
        "deleteIfExists" => Ok(V::Bool(std::fs::remove_file(&path).is_ok())),
        "readAllBytes" => read_file(&path).map(|b| to_byte_array(&b)),
        "readAllLines" => read_file(&path).map(|b| list(lines_of(&b))),
        "readString" => read_file(&path).map(|b| s(&String::from_utf8_lossy(&b))),
        "lines" => read_file(&path).map(|b| V::Stream(Arc::new(Mutex::new(lines_of(&b))))),
        "newInputStream" | "newBufferedReader" => read_file(&path).map(stream),
        "newOutputStream" | "newBufferedWriter" => Ok(opened(path, appends(&a[1..]))),
        "write" | "writeString" => {
            let bytes = match &a[1] {
                V::Arr(_) => bytes_of(&a[1]).unwrap_or_default(),
                V::List(_) => a[1].items().unwrap_or_default().iter().map(|l| l.text() + "\n").collect::<String>().into_bytes(),
                other => other.text().into_bytes(),
            };
            let written = if appends(&a[2..]) {
                use std::io::Write;
                std::fs::OpenOptions::new().append(true).create(true).open(&path).and_then(|mut f| f.write_all(&bytes))
            } else {
                std::fs::write(&path, bytes)
            };
            done(written, a[0].clone())
        }
        "newDirectoryStream" => std::fs::read_dir(&path).map(|entries| list(entries.flatten().map(|e| file(e.path())).collect())).map_err(io_error),
        "list" | "walk" => std::fs::read_dir(&path).map(|entries| V::Stream(Arc::new(Mutex::new(entries.flatten().map(|e| file(e.path())).collect())))).map_err(io_error),
        "copy" => done(std::fs::copy(&path, path_of(&a[1]).unwrap_or_default()).map(|_| ()), a[1].clone()),
        "move" => done(std::fs::rename(&path, path_of(&a[1]).unwrap_or_default()), a[1].clone()),
        _ => return None,
    })
}

pub fn from_json(value: &Value) -> V {
    match value {
        Value::Null => V::Null,
        Value::Bool(b) => V::Bool(*b),
        Value::Number(n) => match n.as_i64() {
            Some(whole) => i32::try_from(whole).map_or(V::Long(whole), V::Int),
            None => V::Double(n.as_f64().unwrap_or(0.0)),
        },
        Value::String(text) => s(text),
        Value::Array(items) => list(items.iter().map(from_json).collect()),
        Value::Object(map) => from_json_map(map),
    }
}

pub fn from_json_map(map: &Map<String, Value>) -> V {
    let out = map.iter().map(|(k, v)| (format!("s{k}"), (s(k), from_json(v)))).collect();
    V::Map(Arc::new(Mutex::new(out)))
}

pub fn to_json(value: &V) -> Value {
    match value {
        V::Null => Value::Null,
        V::Bool(b) => Value::Bool(*b),
        V::Int(n) => Value::from(*n),
        V::Long(n) => Value::from(*n),
        V::Double(n) => Value::from(*n),
        V::Arr(items) | V::List(items) | V::Stream(items) => Value::Array(lock(items).iter().map(to_json).collect()),
        V::Set(map) => Value::Array(lock(map).values().map(|(k, _)| to_json(k)).collect()),
        V::Map(map) => Value::Object(lock(map).values().map(|(k, v)| (k.text(), to_json(v))).collect()),
        other => match other.native::<crate::javautil::JPrim>() {
            Some(primitive) => to_json(&primitive.0),
            None => Value::String(other.text()),
        },
    }
}
