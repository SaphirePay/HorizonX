//! What the plugin corpus reached for and jrt didn't have (the 200-plugin sweep's "not converted" calls): Java's
//! `DataOutput` / `DataInput` (`DataOutputStream`, `DataInputStream`, Guava's `ByteStreams.newDataOutput` and
//! `newDataInput`, which BungeeCord plugin messages are written with), `Calendar` (UTC), `AtomicLongArray` /
//! `AtomicIntegerArray`, and the bit and unsigned helpers of `Double`, `Float`, `Integer` and `Long`.
use crate::{R, V, exc, lock, native, s};
use std::sync::Mutex;

// ---- DataOutput ----

/// What's written so far, and the stream it also goes to (`new DataOutputStream(out)`), if any.
pub struct JDataOut {
    buf: Mutex<Vec<u8>>,
    target: Option<V>,
}

/// Bytes being read, and where the next read starts.
pub struct JDataIn(Mutex<(Vec<u8>, usize)>);

fn data_out(target: Option<V>) -> V {
    native(JDataOut { buf: Mutex::new(Vec::new()), target })
}

fn data_in(bytes: Vec<u8>) -> V {
    native(JDataIn(Mutex::new((bytes, 0))))
}

/// Java's modified UTF-8, as `writeUTF` writes it: a NUL is two bytes, a char past the BMP its surrogates.
fn modified_utf8(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for unit in text.encode_utf16() {
        match unit {
            0x0001..=0x007F => out.push(unit as u8),
            0x0000 | 0x0080..=0x07FF => out.extend([0xC0 | (unit >> 6) as u8, 0x80 | (unit & 0x3F) as u8]),
            _ => out.extend([0xE0 | (unit >> 12) as u8, 0x80 | ((unit >> 6) & 0x3F) as u8, 0x80 | (unit & 0x3F) as u8]),
        }
    }
    out
}

fn read_modified_utf8(bytes: &[u8]) -> String {
    let mut units = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let (unit, step) = if b < 0x80 {
            (u16::from(b), 1)
        } else if b & 0xE0 == 0xC0 && i + 1 < bytes.len() {
            ((u16::from(b & 0x1F) << 6) | u16::from(bytes[i + 1] & 0x3F), 2)
        } else if i + 2 < bytes.len() {
            ((u16::from(b & 0x0F) << 12) | (u16::from(bytes[i + 1] & 0x3F) << 6) | u16::from(bytes[i + 2] & 0x3F), 3)
        } else {
            (u16::from(b), 1)
        };
        units.push(unit);
        i += step;
    }
    String::from_utf16_lossy(&units)
}

fn bytes_arg(value: &V) -> Vec<u8> {
    crate::javaio::bytes_of(value).unwrap_or_default()
}

fn out_method(out: &JDataOut, this: &V, name: &str, a: &[V]) -> Option<R> {
    let piece: Vec<u8> = match (name, a.len()) {
        ("writeInt", 1) => a[0].int().to_be_bytes().to_vec(),
        ("writeLong", 1) => a[0].long().to_be_bytes().to_vec(),
        ("writeShort" | "writeChar", 1) => (a[0].int() as u16).to_be_bytes().to_vec(),
        ("writeByte", 1) | ("write", 1) if !matches!(a[0], V::Arr(_)) && crate::javaio::bytes_of(&a[0]).is_none() || matches!(a[0], V::Int(_) | V::Char(_)) => vec![a[0].int() as u8],
        ("writeBoolean", 1) => vec![u8::from(a[0].truth())],
        ("writeDouble", 1) => a[0].double().to_bits().to_be_bytes().to_vec(),
        ("writeFloat", 1) => (a[0].double() as f32).to_bits().to_be_bytes().to_vec(),
        ("writeUTF", 1) => {
            let text = modified_utf8(&a[0].text());
            if text.len() > 65535 {
                return Some(Err(exc("UTFDataFormatException", "encoded string too long")));
            }
            let mut piece = (text.len() as u16).to_be_bytes().to_vec();
            piece.extend(text);
            piece
        }
        ("writeBytes", 1) => a[0].text().chars().map(|c| c as u32 as u8).collect(),
        ("writeChars", 1) => a[0].text().encode_utf16().flat_map(u16::to_be_bytes).collect(),
        ("write", 1) => bytes_arg(&a[0]),
        ("write", 3) => {
            let all = bytes_arg(&a[0]);
            let (off, len) = (a[1].int().max(0) as usize, a[2].int().max(0) as usize);
            all.get(off..off + len).map(<[u8]>::to_vec).unwrap_or_default()
        }
        ("toByteArray", 0) => return Some(Ok(crate::javaio::to_byte_array(&lock(&out.buf)))),
        ("size", 0) => return Some(Ok(V::Int(lock(&out.buf).len() as i32))),
        ("flush" | "close", 0) => {
            return Some(match &out.target {
                Some(target) => crate::call(target, name, &[]),
                None => Ok(V::Null),
            });
        }
        ("toString", 0) => return Some(Ok(s(&String::from_utf8_lossy(&lock(&out.buf))))),
        _ => return None,
    };
    lock(&out.buf).extend(&piece);
    if let Some(target) = &out.target {
        if let Err(error) = crate::call(target, "write", &[crate::javaio::to_byte_array(&piece)]) {
            return Some(Err(error));
        }
    }
    let _ = this;
    Some(Ok(V::Null))
}

fn take(input: &JDataIn, n: usize) -> Result<Vec<u8>, V> {
    let mut state = lock(&input.0);
    let (bytes, at) = &mut *state;
    if *at + n > bytes.len() {
        *at = bytes.len();
        return Err(exc("EOFException", ""));
    }
    let piece = bytes[*at..*at + n].to_vec();
    *at += n;
    Ok(piece)
}

fn in_method(input: &JDataIn, name: &str, a: &[V]) -> Option<R> {
    let fixed = |n: usize| take(input, n);
    Some(match (name, a.len()) {
        ("readInt", 0) => fixed(4).map(|b| V::Int(i32::from_be_bytes(b.try_into().unwrap_or_default()))),
        ("readLong", 0) => fixed(8).map(|b| V::Long(i64::from_be_bytes(b.try_into().unwrap_or_default()))),
        ("readShort", 0) => fixed(2).map(|b| V::Int(i32::from(i16::from_be_bytes([b[0], b[1]])))),
        ("readUnsignedShort", 0) => fixed(2).map(|b| V::Int(i32::from(u16::from_be_bytes([b[0], b[1]])))),
        ("readChar", 0) => fixed(2).map(|b| V::Char(char::from_u32(u32::from(u16::from_be_bytes([b[0], b[1]]))).unwrap_or('\u{fffd}'))),
        ("readByte", 0) => fixed(1).map(|b| V::Int(i32::from(b[0] as i8))),
        ("readUnsignedByte", 0) => fixed(1).map(|b| V::Int(i32::from(b[0]))),
        // `InputStream.read()`: -1 at the end, not an exception.
        ("read", 0) => Ok(fixed(1).map_or(V::Int(-1), |b| V::Int(i32::from(b[0])))),
        ("readBoolean", 0) => fixed(1).map(|b| V::Bool(b[0] != 0)),
        ("readDouble", 0) => fixed(8).map(|b| V::Double(f64::from_bits(u64::from_be_bytes(b.try_into().unwrap_or_default())))),
        ("readFloat", 0) => fixed(4).map(|b| V::Double(f64::from(f32::from_bits(u32::from_be_bytes(b.try_into().unwrap_or_default()))))),
        ("readUTF", 0) => fixed(2).and_then(|n| fixed(usize::from(u16::from_be_bytes([n[0], n[1]])))).map(|b| s(&read_modified_utf8(&b))),
        ("readFully", 1) | ("read", 1) => {
            let V::Arr(target) = &a[0] else { return None };
            let want = lock(target).len();
            let available = { let state = lock(&input.0); state.0.len() - state.1 };
            if name == "read" && available == 0 && want > 0 {
                return Some(Ok(V::Int(-1)));
            }
            let n = if name == "read" { want.min(available) } else { want };
            fixed(n).map(|bytes| {
                let mut items = lock(target);
                for (slot, b) in items.iter_mut().zip(bytes) {
                    *slot = V::Int(i32::from(b as i8));
                }
                V::Int(n as i32)
            })
        }
        ("skipBytes", 1) => {
            let mut state = lock(&input.0);
            let n = (a[0].int().max(0) as usize).min(state.0.len() - state.1);
            state.1 += n;
            Ok(V::Int(n as i32))
        }
        ("available", 0) => Ok({ let state = lock(&input.0); V::Int((state.0.len() - state.1) as i32) }),
        ("readLine", 0) => {
            let mut state = lock(&input.0);
            let (bytes, at) = &mut *state;
            if *at >= bytes.len() {
                Ok(V::Null)
            } else {
                let end = bytes[*at..].iter().position(|b| *b == b'\n').map_or(bytes.len(), |p| *at + p);
                let line = String::from_utf8_lossy(&bytes[*at..end]).trim_end_matches('\r').to_owned();
                *at = (end + 1).min(bytes.len());
                Ok(s(&line))
            }
        }
        ("close", 0) => Ok(V::Null),
        _ => return None,
    })
}

// ---- Calendar, in UTC ----

pub struct JCalendar(Mutex<i64>);

const DAY_MS: i64 = 86_400_000;

/// Days since 1970-01-01 to (year, month 1..12, day): Howard Hinnant's civil-from-days.
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The fields `Calendar.get` knows: YEAR 1, MONTH 2 (0-based), DAY_OF_MONTH 5, DAY_OF_YEAR 6, DAY_OF_WEEK 7 (Sunday 1),
/// HOUR 10, HOUR_OF_DAY 11, MINUTE 12, SECOND 13, MILLISECOND 14, AM_PM 9.
fn calendar_get(ms: i64, field: i32) -> i64 {
    let days = ms.div_euclid(DAY_MS);
    let in_day = ms.rem_euclid(DAY_MS);
    let (y, m, d) = civil(days);
    match field {
        1 => y,
        2 => m - 1,
        5 => d,
        6 => days - days_from_civil(y, 1, 1) + 1,
        7 => (days + 4).rem_euclid(7) + 1,
        9 => i64::from(in_day >= DAY_MS / 2),
        10 => (in_day / 3_600_000) % 12,
        11 => in_day / 3_600_000,
        12 => (in_day / 60_000) % 60,
        13 => (in_day / 1000) % 60,
        14 => in_day % 1000,
        _ => 0,
    }
}

fn calendar_set(ms: i64, field: i32, value: i64) -> i64 {
    let days = ms.div_euclid(DAY_MS);
    let in_day = ms.rem_euclid(DAY_MS);
    let (y, m, d) = civil(days);
    let at = |y: i64, m: i64, d: i64| days_from_civil(y, m, d) * DAY_MS + in_day;
    match field {
        1 => at(value, m, d),
        2 => {
            let total = y * 12 + value;
            at(total.div_euclid(12), total.rem_euclid(12) + 1, d)
        }
        5 => at(y, m, 1) + (value - 1) * DAY_MS,
        11 => days * DAY_MS + value * 3_600_000 + in_day % 3_600_000,
        12 => ms - calendar_get(ms, 12) * 60_000 + value * 60_000,
        13 => ms - calendar_get(ms, 13) * 1000 + value * 1000,
        14 => ms - calendar_get(ms, 14) + value,
        _ => ms,
    }
}

fn calendar_add(ms: i64, field: i32, amount: i64) -> i64 {
    match field {
        1 => calendar_set(ms, 1, calendar_get(ms, 1) + amount),
        2 => calendar_set(ms, 2, calendar_get(ms, 2) + amount),
        5 | 6 | 7 => ms + amount * DAY_MS,
        3 | 4 => ms + amount * 7 * DAY_MS,
        10 | 11 => ms + amount * 3_600_000,
        12 => ms + amount * 60_000,
        13 => ms + amount * 1000,
        14 => ms + amount,
        _ => ms,
    }
}

fn calendar_method(calendar: &JCalendar, this: &V, name: &str, a: &[V]) -> Option<R> {
    let mut ms = lock(&calendar.0);
    Some(Ok(match (name, a.len()) {
        ("getTimeInMillis", 0) => V::Long(*ms),
        ("setTimeInMillis", 1) => {
            *ms = a[0].long();
            V::Null
        }
        ("getTime", 0) => crate::javautil::instant(*ms),
        ("setTime", 1) => {
            *ms = crate::call(&a[0], "getTime", &[]).map(|v| v.long()).unwrap_or(*ms);
            V::Null
        }
        ("get", 1) => V::Int(calendar_get(*ms, a[0].int()) as i32),
        ("set", 2) => {
            *ms = calendar_set(*ms, a[0].int(), a[1].long());
            V::Null
        }
        ("set", 3) | ("set", 5) | ("set", 6) => {
            let (y, m, d) = (a[0].long(), a[1].long(), a[2].long());
            let in_day = if a.len() >= 5 { a[3].long() * 3_600_000 + a[4].long() * 60_000 + a.get(5).map_or(0, V::long) * 1000 } else { ms.rem_euclid(DAY_MS) };
            let total = y * 12 + m;
            *ms = days_from_civil(total.div_euclid(12), total.rem_euclid(12) + 1, 1) * DAY_MS + (d - 1) * DAY_MS + in_day;
            V::Null
        }
        ("add", 2) => {
            *ms = calendar_add(*ms, a[0].int(), a[1].long());
            V::Null
        }
        ("getActualMaximum", 1) if a[0].int() == 5 => {
            let (y, m, _) = civil(ms.div_euclid(DAY_MS));
            let next = if m == 12 { days_from_civil(y + 1, 1, 1) } else { days_from_civil(y, m + 1, 1) };
            V::Int((next - days_from_civil(y, m, 1)) as i32)
        }
        ("toInstant", 0) => crate::javautil::instant(*ms),
        ("getTimeZone", 0) => s("UTC"),
        ("setTimeZone" | "setLenient" | "setFirstDayOfWeek", 1) => V::Null,
        ("clone", 0) => native(JCalendar(Mutex::new(*ms))),
        ("before", 1) | ("after", 1) | ("compareTo", 1) => {
            let other = a[0].native::<JCalendar>().map_or(0, |c| *lock(&c.0));
            match name {
                "before" => V::Bool(*ms < other),
                "after" => V::Bool(*ms > other),
                _ => V::Int((*ms).cmp(&other) as i32),
            }
        }
        ("equals", 1) => V::Bool(a[0].native::<JCalendar>().is_some_and(|c| std::ptr::eq(c, calendar) || *lock(&c.0) == *ms)),
        ("hashCode", 0) => V::Int((*ms ^ (*ms >> 32)) as i32),
        ("toString", 0) => s(&format!("Calendar[time={}]", *ms)),
        _ => {
            let _ = this;
            return None;
        }
    }))
}

// ---- AtomicLongArray, AtomicIntegerArray ----

pub struct JAtomicArray {
    items: Mutex<Vec<i64>>,
    long: bool,
}

fn atomic_value(array: &JAtomicArray, value: i64) -> V {
    if array.long { V::Long(value) } else { V::Int(value as i32) }
}

fn atomic_method(array: &JAtomicArray, name: &str, a: &[V]) -> Option<R> {
    let mut items = lock(&array.items);
    let index = |at: &V| -> Result<usize, V> {
        let i = at.int();
        if i < 0 || i as usize >= items.len() {
            return Err(exc("IndexOutOfBoundsException", format!("index {i}")));
        }
        Ok(i as usize)
    };
    let wrap = |n: i64| if array.long { n } else { i64::from(n as i32) };
    Some(match (name, a.len()) {
        ("length", 0) => Ok(V::Int(items.len() as i32)),
        ("get" | "getPlain" | "getAcquire" | "getOpaque", 1) => index(&a[0]).map(|i| atomic_value(array, items[i])),
        ("set" | "lazySet" | "setPlain" | "setRelease" | "setOpaque", 2) => index(&a[0]).map(|i| {
            items[i] = wrap(a[1].long());
            V::Null
        }),
        ("getAndSet", 2) => index(&a[0]).map(|i| atomic_value(array, std::mem::replace(&mut items[i], wrap(a[1].long())))),
        ("incrementAndGet" | "decrementAndGet" | "getAndIncrement" | "getAndDecrement", 1) => index(&a[0]).map(|i| {
            let before = items[i];
            items[i] = wrap(before.wrapping_add(if name.contains("ncrement") { 1 } else { -1 }));
            atomic_value(array, if name.starts_with("get") { before } else { items[i] })
        }),
        ("addAndGet" | "getAndAdd", 2) => index(&a[0]).map(|i| {
            let before = items[i];
            items[i] = wrap(before.wrapping_add(a[1].long()));
            atomic_value(array, if name == "getAndAdd" { before } else { items[i] })
        }),
        ("compareAndSet" | "weakCompareAndSet", 3) => index(&a[0]).map(|i| {
            let done = items[i] == a[1].long();
            if done {
                items[i] = wrap(a[2].long());
            }
            V::Bool(done)
        }),
        ("toString", 0) => Ok(s(&format!("[{}]", items.iter().map(i64::to_string).collect::<Vec<_>>().join(", ")))),
        _ => return None,
    })
}

// ---- The hooks jrt calls ----

pub fn construct(class: &str, a: &[V]) -> Option<R> {
    Some(Ok(match class {
        "DataOutputStream" | "ObjectOutputStream" => data_out(a.first().cloned()),
        "DataInputStream" | "ObjectInputStream" => data_in(a.first().map(bytes_arg).unwrap_or_default()),
        "GregorianCalendar" if a.is_empty() => native(JCalendar(Mutex::new(crate::javautil::now_ms()))),
        "GregorianCalendar" if a.len() >= 3 => {
            let calendar = JCalendar(Mutex::new(0));
            let this = native(JCalendar(Mutex::new(0)));
            let _ = calendar_method(&calendar, &this, "set", a);
            native(calendar)
        }
        "AtomicLongArray" | "AtomicIntegerArray" => {
            let items = match a.first() {
                Some(V::Arr(values)) => lock(values).iter().map(V::long).collect(),
                Some(length) => vec![0; length.int().max(0) as usize],
                None => Vec::new(),
            };
            native(JAtomicArray { items: Mutex::new(items), long: class == "AtomicLongArray" })
        }
        _ => return None,
    }))
}

pub fn static_call(class: &str, name: &str, a: &[V]) -> Option<R> {
    let unsigned = |x: &V| x.long() as u64;
    Some(Ok(match (class, name) {
        ("ByteStreams", "newDataOutput") => data_out(None),
        ("ByteStreams", "newDataInput") => {
            let bytes = bytes_arg(&a[0]);
            let start = a.get(1).map_or(0, |v| v.int().max(0) as usize).min(bytes.len());
            data_in(bytes[start..].to_vec())
        }
        ("Calendar", "getInstance") => native(JCalendar(Mutex::new(crate::javautil::now_ms()))),
        ("Double", "doubleToLongBits") => {
            let x = a[0].double();
            V::Long(if x.is_nan() { 0x7ff8_0000_0000_0000 } else { x.to_bits() as i64 })
        }
        ("Double", "doubleToRawLongBits") => V::Long(a[0].double().to_bits() as i64),
        ("Double", "longBitsToDouble") => V::Double(f64::from_bits(a[0].long() as u64)),
        ("Float", "floatToIntBits") => {
            let x = a[0].double() as f32;
            V::Int(if x.is_nan() { 0x7fc0_0000 } else { x.to_bits() as i32 })
        }
        ("Float", "floatToRawIntBits") => V::Int((a[0].double() as f32).to_bits() as i32),
        ("Float", "intBitsToFloat") => V::Double(f64::from(f32::from_bits(a[0].int() as u32))),
        ("Long", "compareUnsigned") => V::Int(unsigned(&a[0]).cmp(&unsigned(&a[1])) as i32),
        ("Integer", "compareUnsigned") => V::Int((a[0].int() as u32).cmp(&(a[1].int() as u32)) as i32),
        ("Integer", "toUnsignedLong") => V::Long(i64::from(a[0].int() as u32)),
        ("Integer", "toUnsignedString") => s(&(a[0].int() as u32).to_string()),
        ("Long", "toUnsignedString") => s(&unsigned(&a[0]).to_string()),
        ("Integer", "divideUnsigned") if a[1].int() != 0 => V::Int(((a[0].int() as u32) / (a[1].int() as u32)) as i32),
        ("Integer", "remainderUnsigned") if a[1].int() != 0 => V::Int(((a[0].int() as u32) % (a[1].int() as u32)) as i32),
        ("Long", "divideUnsigned") if a[1].long() != 0 => V::Long((unsigned(&a[0]) / unsigned(&a[1])) as i64),
        ("Long", "remainderUnsigned") if a[1].long() != 0 => V::Long((unsigned(&a[0]) % unsigned(&a[1])) as i64),
        _ => return None,
    }))
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if let Some(out) = this.native::<JDataOut>() {
        return out_method(out, this, name, a);
    }
    if let Some(input) = this.native::<JDataIn>() {
        return in_method(input, name, a);
    }
    if let Some(calendar) = this.native::<JCalendar>() {
        return calendar_method(calendar, this, name, a);
    }
    if let Some(array) = this.native::<JAtomicArray>() {
        return atomic_method(array, name, a);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(r: Option<R>) -> V {
        match r {
            Some(Ok(v)) => v,
            _ => panic!("no value"),
        }
    }

    #[test]
    fn data_out_and_in_round_trip_as_java_writes_them() {
        let out = data_out(None);
        for (name, value) in [("writeUTF", s("Connect")), ("writeInt", V::Int(-2)), ("writeLong", V::Long(1 << 40)), ("writeBoolean", V::Bool(true)), ("writeShort", V::Int(65535)), ("writeDouble", V::Double(1.5))] {
            ok(method(&out, name, &[value]));
        }
        let bytes = crate::javaio::bytes_of(&ok(method(&out, "toByteArray", &[]))).unwrap();
        assert_eq!(&bytes[..9], &[0, 7, b'C', b'o', b'n', b'n', b'e', b'c', b't']);
        let input = data_in(bytes);
        let read = |name: &str| ok(method(&input, name, &[]));
        assert_eq!(read("readUTF").text(), "Connect");
        assert_eq!(read("readInt").int(), -2);
        assert_eq!(read("readLong").long(), 1 << 40);
        assert!(read("readBoolean").truth());
        assert_eq!(read("readUnsignedShort").int(), 65535);
        assert_eq!(read("readDouble").double(), 1.5);
        assert!(method(&input, "readByte", &[]).unwrap().is_err());
    }

    #[test]
    fn a_nul_is_two_bytes_in_modified_utf8() {
        assert_eq!(modified_utf8("a\0"), vec![b'a', 0xC0, 0x80]);
        assert_eq!(read_modified_utf8(&modified_utf8("é€\0x")), "é€\0x");
    }

    #[test]
    fn calendar_fields_are_utc_dates() {
        // 2024-02-29 13:45:30.250 UTC.
        let ms = days_from_civil(2024, 2, 29) * DAY_MS + 13 * 3_600_000 + 45 * 60_000 + 30_250;
        assert_eq!([1, 2, 5, 11, 12, 13, 14, 7].map(|f| calendar_get(ms, f)), [2024, 1, 29, 13, 45, 30, 250, 5]);
        assert_eq!(calendar_get(calendar_add(ms, 2, 12), 1), 2025);
        assert_eq!(calendar_get(calendar_add(ms, 5, 1), 2), 2);
    }

    #[test]
    fn atomic_arrays_count() {
        let array = ok(construct("AtomicLongArray", &[V::Int(3)]));
        assert_eq!(ok(method(&array, "incrementAndGet", &[V::Int(1)])).long(), 1);
        assert_eq!(ok(method(&array, "addAndGet", &[V::Int(1), V::Long(5)])).long(), 6);
        assert_eq!(ok(method(&array, "length", &[])).int(), 3);
        assert!(method(&array, "get", &[V::Int(3)]).unwrap().is_err());
    }

    #[test]
    fn bits_and_unsigned() {
        assert_eq!(ok(static_call("Double", "doubleToLongBits", &[V::Double(1.0)])).long(), 0x3ff0_0000_0000_0000);
        assert_eq!(ok(static_call("Integer", "compareUnsigned", &[V::Int(-1), V::Int(1)])).int(), 1);
        assert_eq!(ok(static_call("Long", "compareUnsigned", &[V::Long(1), V::Long(-1)])).int(), -1);
    }
}
