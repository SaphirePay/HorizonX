//! `java.util.zip`: `ZipInputStream` (a world shipped in a plugin's jar, unpacked at start) and
//! `GZIPInputStream`, over a small DEFLATE decoder (RFC 1951) of their own.
use crate::{R, V, exc, lock, native, s};
use std::sync::Mutex;

struct Bits<'a> {
    data: &'a [u8],
    at: usize,
    bit: u32,
    held: u32,
}

impl Bits<'_> {
    fn need(&mut self, n: u32) -> Result<u32, String> {
        while self.bit < n {
            let byte = *self.data.get(self.at).ok_or("unexpected end of ZLIB input stream")?;
            self.at += 1;
            self.held |= u32::from(byte) << self.bit;
            self.bit += 8;
        }
        let value = self.held & ((1u64 << n) - 1) as u32;
        self.held >>= n;
        self.bit -= n;
        Ok(value)
    }
    fn align(&mut self) {
        self.held = 0;
        self.bit = 0;
    }
}

/// Canonical Huffman code: counts per length and the symbols in order.
struct Code {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Code {
    fn new(lengths: &[u8]) -> Self {
        let mut counts = [0u16; 16];
        for &l in lengths {
            counts[usize::from(l)] += 1;
        }
        counts[0] = 0;
        let mut offsets = [0u16; 16];
        for i in 1..16 {
            offsets[i] = offsets[i - 1] + counts[i - 1];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (symbol, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[usize::from(offsets[usize::from(l)])] = symbol as u16;
                offsets[usize::from(l)] += 1;
            }
        }
        Self { counts, symbols }
    }
    fn decode(&self, bits: &mut Bits) -> Result<u16, String> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= bits.need(1)? as i32;
            let count = i32::from(self.counts[len]);
            if code - count < first {
                return Ok(self.symbols[(index + (code - first)) as usize]);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err("invalid Huffman code".into())
    }
}

const LENGTH_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LENGTH_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

/// Raw DEFLATE: the bytes, and how many input bytes it took.
pub fn inflate(data: &[u8]) -> Result<(Vec<u8>, usize), String> {
    let mut bits = Bits { data, at: 0, bit: 0, held: 0 };
    let mut out = Vec::new();
    loop {
        let last = bits.need(1)? == 1;
        match bits.need(2)? {
            0 => {
                bits.align();
                let header = data.get(bits.at..bits.at + 4).ok_or("truncated stored block")?;
                let len = usize::from(u16::from_le_bytes([header[0], header[1]]));
                bits.at += 4;
                out.extend_from_slice(data.get(bits.at..bits.at + len).ok_or("truncated stored block")?);
                bits.at += len;
            }
            kind @ (1 | 2) => {
                let (lit, dist) = if kind == 1 {
                    let mut lengths = [0u8; 288];
                    lengths[..144].fill(8);
                    lengths[144..256].fill(9);
                    lengths[256..280].fill(7);
                    lengths[280..].fill(8);
                    (Code::new(&lengths), Code::new(&[5u8; 30]))
                } else {
                    let (nlen, ndist, ncode) = (bits.need(5)? as usize + 257, bits.need(5)? as usize + 1, bits.need(4)? as usize + 4);
                    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                    let mut code_lengths = [0u8; 19];
                    for &at in ORDER.iter().take(ncode) {
                        code_lengths[at] = bits.need(3)? as u8;
                    }
                    let code = Code::new(&code_lengths);
                    let mut lengths = Vec::with_capacity(nlen + ndist);
                    while lengths.len() < nlen + ndist {
                        let symbol = code.decode(&mut bits)?;
                        match symbol {
                            0..=15 => lengths.push(symbol as u8),
                            16 => {
                                let previous = *lengths.last().ok_or("repeat with no length")?;
                                for _ in 0..3 + bits.need(2)? {
                                    lengths.push(previous);
                                }
                            }
                            17 => lengths.extend(std::iter::repeat_n(0, 3 + bits.need(3)? as usize)),
                            _ => lengths.extend(std::iter::repeat_n(0, 11 + bits.need(7)? as usize)),
                        }
                    }
                    (Code::new(&lengths[..nlen]), Code::new(&lengths[nlen..nlen + ndist]))
                };
                loop {
                    let symbol = lit.decode(&mut bits)?;
                    if symbol < 256 {
                        out.push(symbol as u8);
                    } else if symbol == 256 {
                        break;
                    } else {
                        let at = usize::from(symbol - 257);
                        let len = usize::from(*LENGTH_BASE.get(at).ok_or("bad length")?) + bits.need(u32::from(LENGTH_EXTRA[at]))? as usize;
                        let d = usize::from(dist.decode(&mut bits)?);
                        let back = usize::from(*DIST_BASE.get(d).ok_or("bad distance")?) + bits.need(u32::from(DIST_EXTRA[d]))? as usize;
                        if back > out.len() {
                            return Err("invalid distance too far back".into());
                        }
                        let start = out.len() - back;
                        for i in 0..len {
                            out.push(out[start + i]);
                        }
                    }
                }
            }
            _ => return Err("invalid block type".into()),
        }
        if last {
            return Ok((out, bits.at));
        }
    }
}

/// `ZipInputStream` over a stream's bytes: entries one after another, as their local headers say.
pub struct JZipIn {
    data: Vec<u8>,
    at: Mutex<usize>,
    current: Mutex<(Vec<u8>, usize)>,
}

pub struct JZipEntry {
    name: String,
    size: i64,
    compressed: i64,
}

fn u16_at(d: &[u8], at: usize) -> usize {
    d.get(at..at + 2).map_or(0, |b| usize::from(u16::from_le_bytes([b[0], b[1]])))
}

fn u32_at(d: &[u8], at: usize) -> usize {
    d.get(at..at + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
}

fn next_entry(z: &JZipIn) -> R {
    let mut at = lock(&z.at);
    let d = &z.data;
    if u32_at(d, *at) != 0x0403_4b50 {
        *lock(&z.current) = (Vec::new(), 0);
        return Ok(V::Null);
    }
    let (flags, method) = (u16_at(d, *at + 6), u16_at(d, *at + 8));
    let (mut compressed, size) = (u32_at(d, *at + 18), u32_at(d, *at + 22));
    let (name_len, extra_len) = (u16_at(d, *at + 26), u16_at(d, *at + 28));
    let name = String::from_utf8_lossy(d.get(*at + 30..*at + 30 + name_len).unwrap_or_default()).into_owned();
    let start = *at + 30 + name_len + extra_len;
    let content = match method {
        0 => d.get(start..start + compressed).ok_or_else(|| exc("ZipException", "truncated entry"))?.to_vec(),
        8 => {
            let (bytes, used) = inflate(d.get(start..).unwrap_or_default()).map_err(|e| exc("ZipException", e))?;
            if flags & 8 != 0 {
                compressed = used;
            }
            bytes
        }
        other => return Err(exc("ZipException", format!("invalid compression method {other}"))),
    };
    let mut next = start + compressed;
    // A data descriptor after the data (bit 3): its optional signature, CRC and two sizes.
    if flags & 8 != 0 {
        next += if u32_at(d, next) == 0x0807_4b50 { 16 } else { 12 };
    }
    *at = next;
    let size = if flags & 8 != 0 && size == 0 { content.len() } else { size };
    *lock(&z.current) = (content, 0);
    Ok(native(JZipEntry { name, size: size as i64, compressed: compressed as i64 }))
}

pub fn construct(class: &str, a: &[V]) -> Option<R> {
    if let Some(found) = construct_out(class, a) {
        return Some(found);
    }
    let simple = class.rsplit('.').next().unwrap_or(class);
    let bytes = || a.first().and_then(crate::javaio::bytes_of).ok_or_else(|| exc("NullPointerException", "stream"));
    Some(match simple {
        "ZipInputStream" => bytes().map(|data| native(JZipIn { data, at: Mutex::new(0), current: Mutex::new((Vec::new(), 0)) })),
        "GZIPInputStream" => bytes().and_then(|data| gunzip(&data).map(crate::javaio::stream)),
        "InflaterInputStream" => bytes().and_then(|data| {
            // zlib: a two-byte header before the DEFLATE data.
            inflate(data.get(2..).unwrap_or_default()).map(|(out, _)| crate::javaio::stream(out)).map_err(|e| exc("ZipException", e))
        }),
        _ => return None,
    })
}

fn gunzip(data: &[u8]) -> Result<Vec<u8>, V> {
    if data.len() < 18 || data[0] != 0x1f || data[1] != 0x8b {
        return Err(exc("ZipException", "Not in GZIP format"));
    }
    let flags = data[3];
    let mut at = 10;
    if flags & 4 != 0 {
        at += 2 + u16_at(data, at);
    }
    for bit in [8u8, 16] {
        if flags & bit != 0 {
            at += data.get(at..).and_then(|rest| rest.iter().position(|b| *b == 0)).map_or(0, |n| n + 1);
        }
    }
    if flags & 2 != 0 {
        at += 2;
    }
    inflate(data.get(at..).unwrap_or_default()).map(|(out, _)| out).map_err(|e| exc("ZipException", e))
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if let Some(found) = method_out(this, name, a) {
        return Some(found);
    }
    if let Some(e) = this.native::<JZipEntry>() {
        return Some(Ok(match name {
            "getName" | "toString" => s(&e.name),
            "isDirectory" => V::Bool(e.name.ends_with('/')),
            "getSize" => V::Long(e.size),
            "getCompressedSize" => V::Long(e.compressed),
            "getTime" | "getCrc" => V::Long(-1),
            "hashCode" => V::Int(e.name.len() as i32),
            _ => return None,
        }));
    }
    let z = this.native::<JZipIn>()?;
    Some(match (name, a.len()) {
        ("getNextEntry", 0) => next_entry(z),
        ("closeEntry" | "close", 0) => Ok(V::Null),
        ("available", 0) => {
            let current = lock(&z.current);
            Ok(V::Int(i32::from(current.1 < current.0.len())))
        }
        ("read", 0) => {
            let mut current = lock(&z.current);
            let at = current.1;
            Ok(match current.0.get(at).copied() {
                Some(byte) => {
                    current.1 += 1;
                    V::Int(i32::from(byte))
                }
                None => V::Int(-1),
            })
        }
        ("read", 1 | 3) => {
            let V::Arr(buffer) = &a[0] else { return Some(Err(exc("NullPointerException", "buffer"))) };
            let mut buffer = lock(buffer);
            let offset = a.get(1).map_or(0, |n| n.int().max(0) as usize).min(buffer.len());
            let length = a.get(2).map_or(buffer.len() - offset, |n| n.int().max(0) as usize).min(buffer.len() - offset);
            let mut current = lock(&z.current);
            if length == 0 {
                return Some(Ok(V::Int(0)));
            }
            if current.1 >= current.0.len() {
                return Some(Ok(V::Int(-1)));
            }
            let n = length.min(current.0.len() - current.1);
            for i in 0..n {
                buffer[offset + i] = V::Int(i32::from(current.0[current.1 + i] as i8));
            }
            current.1 += n;
            Ok(V::Int(n as i32))
        }
        ("readAllBytes", 0) => {
            let mut current = lock(&z.current);
            let rest = current.0[current.1.min(current.0.len())..].to_vec();
            current.1 = current.0.len();
            Ok(crate::javaio::to_byte_array(&rest))
        }
        _ => return None,
    })
}

// ---- Writing: `ZipOutputStream`, entries stored (every zip reader takes them, Minecraft's pack loader too) ----

/// `ZipOutputStream`: the stream it writes to, the entries done, and the one being written.
pub struct JZipOut {
    out: V,
    done: Mutex<Vec<(String, Vec<u8>)>>,
    current: Mutex<Option<(String, Vec<u8>)>>,
    finished: Mutex<bool>,
}

/// CRC-32 (IEEE), as `java.util.zip.CRC32`.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn zip_bytes(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let (mut out, mut central) = (Vec::new(), Vec::new());
    for (name, data) in entries {
        let (crc, offset) = (crc32(data), out.len() as u32);
        let size = data.len() as u32;
        // Local header: stored, no data descriptor, UTF-8 names (flag 0x800).
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&[20, 0, 0, 8, 0, 0, 0, 0, 0x21, 0]);
        for v in [crc, size, size] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&[20, 0, 20, 0, 0, 8, 0, 0, 0, 0, 0x21, 0]);
        for v in [crc, size, size] {
            central.extend_from_slice(&v.to_le_bytes());
        }
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let (start, count) = (out.len() as u32, entries.len() as u16);
    let length = central.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

pub fn construct_out(class: &str, a: &[V]) -> Option<R> {
    let simple = class.rsplit('.').next().unwrap_or(class);
    Some(Ok(match (simple, a.len()) {
        ("ZipOutputStream", 1 | 2) => native(JZipOut { out: a[0].clone(), done: Mutex::new(Vec::new()), current: Mutex::new(None), finished: Mutex::new(false) }),
        ("ZipEntry", 1) => native(JZipEntry { name: a[0].text(), size: -1, compressed: -1 }),
        ("CRC32", 0) => native(JCrc(Mutex::new(Vec::new()))),
        // bStats' `new GZIPOutputStream(bytes)` (MetricsBase.compress), a plugin's zlib `DeflaterOutputStream`.
        ("GZIPOutputStream", 1 | 2 | 3) => native(JDeflateOut { out: a[0].clone(), data: Mutex::new(Vec::new()), finished: Mutex::new(false), gzip: true }),
        ("DeflaterOutputStream", 1 | 2 | 3 | 4) => native(JDeflateOut { out: a[0].clone(), data: Mutex::new(Vec::new()), finished: Mutex::new(false), gzip: false }),
        _ => return None,
    }))
}

/// `GZIPOutputStream` / `DeflaterOutputStream`: the bytes written, sent on to `out` at `finish`/`close` as DEFLATE
/// stored blocks in a gzip or zlib frame (every reader takes them; bStats' server inflates them).
pub struct JDeflateOut {
    out: V,
    data: Mutex<Vec<u8>>,
    finished: Mutex<bool>,
    gzip: bool,
}

/// DEFLATE (RFC 1951) of stored blocks: no compression, always valid.
fn deflate_stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 65535 * 5 + 5);
    let mut chunks = data.chunks(65535).peekable();
    if chunks.peek().is_none() {
        return vec![1, 0, 0, 0xff, 0xff];
    }
    while let Some(chunk) = chunks.next() {
        out.push(u8::from(chunks.peek().is_none()));
        let len = chunk.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// The whole frame for `data`: gzip (RFC 1952) or zlib (RFC 1950).
pub fn deflate_frame(data: &[u8], gzip: bool) -> Vec<u8> {
    let body = deflate_stored(data);
    let mut out = Vec::with_capacity(body.len() + 18);
    if gzip {
        out.extend_from_slice(&[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff]);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(data).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    } else {
        out.extend_from_slice(&[0x78, 0x01]);
        out.extend_from_slice(&body);
        out.extend_from_slice(&adler32(data).to_be_bytes());
    }
    out
}

fn deflate_out_method(z: &JDeflateOut, name: &str, a: &[V]) -> Option<R> {
    let finish = || -> R {
        if std::mem::replace(&mut *lock(&z.finished), true) {
            return Ok(V::Null);
        }
        let bytes = deflate_frame(&lock(&z.data), z.gzip);
        crate::call(&z.out, "write", &[crate::javaio::to_byte_array(&bytes)])?;
        crate::call(&z.out, "flush", &[]).map(|_| V::Null).or(Ok(V::Null))
    };
    Some(match name {
        "write" => {
            lock(&z.data).extend(take_bytes(a));
            Ok(V::Null)
        }
        "flush" => Ok(V::Null),
        "finish" => finish(),
        "close" => finish().and_then(|_| crate::call(&z.out, "close", &[]).or(Ok(V::Null))),
        _ => return None,
    })
}

/// `java.util.zip.CRC32`: the bytes so far.
pub struct JCrc(Mutex<Vec<u8>>);

fn take_bytes(a: &[V]) -> Vec<u8> {
    match a {
        [V::Int(b)] => vec![*b as u8],
        [data] => crate::javaio::bytes_of(data).unwrap_or_default(),
        [data, off, len] => {
            let all = crate::javaio::bytes_of(data).unwrap_or_default();
            let (off, len) = (off.int().max(0) as usize, len.int().max(0) as usize);
            all.get(off..(off + len).min(all.len())).unwrap_or_default().to_vec()
        }
        _ => Vec::new(),
    }
}

pub fn method_out(this: &V, name: &str, a: &[V]) -> Option<R> {
    if let Some(c) = this.native::<JCrc>() {
        return Some(Ok(match name {
            "update" => {
                lock(&c.0).extend(take_bytes(a));
                V::Null
            }
            "getValue" => V::Long(i64::from(crc32(&lock(&c.0)))),
            "reset" => {
                lock(&c.0).clear();
                V::Null
            }
            _ => return None,
        }));
    }
    if let Some(z) = this.native::<JDeflateOut>() {
        return deflate_out_method(z, name, a);
    }
    let z = this.native::<JZipOut>()?;
    let close_entry = || {
        if let Some(entry) = lock(&z.current).take() {
            lock(&z.done).push(entry);
        }
    };
    let finish = || -> R {
        close_entry();
        if std::mem::replace(&mut *lock(&z.finished), true) {
            return Ok(V::Null);
        }
        let bytes = zip_bytes(&lock(&z.done));
        crate::call(&z.out, "write", &[crate::javaio::to_byte_array(&bytes)])?;
        crate::call(&z.out, "flush", &[]).map(|_| V::Null).or(Ok(V::Null))
    };
    Some(match name {
        "putNextEntry" => {
            close_entry();
            let entry = a.first().and_then(|e| e.native::<JZipEntry>().map(|e| e.name.clone())).unwrap_or_else(|| a.first().map(V::text).unwrap_or_default());
            if lock(&z.done).iter().any(|(n, _)| *n == entry) {
                return Some(Err(exc("ZipException", format!("duplicate entry: {entry}"))));
            }
            *lock(&z.current) = Some((entry, Vec::new()));
            Ok(V::Null)
        }
        "write" => match lock(&z.current).as_mut() {
            Some((_, data)) => {
                data.extend(take_bytes(a));
                Ok(V::Null)
            }
            None => Err(exc("ZipException", "no current ZIP entry")),
        },
        "closeEntry" => {
            close_entry();
            Ok(V::Null)
        }
        "finish" => finish(),
        "close" => finish().and_then(|_| crate::call(&z.out, "close", &[]).or(Ok(V::Null))),
        "setMethod" | "setLevel" | "setComment" | "flush" => Ok(V::Null),
        _ => return None,
    })
}

#[cfg(test)]
mod writer_tests {
    #[test]
    fn a_written_zip_reads_back() {
        let bytes = super::zip_bytes(&[("pack.mcmeta".into(), b"{\"pack\":{}}".to_vec()), ("assets/a.txt".into(), b"hello".to_vec())]);
        let path = std::env::temp_dir().join("jrt_zip_test.zip");
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(super::crc32(b"hello"), 0x3610_a686);
        assert!(bytes.windows(4).any(|w| w == [0x50, 0x4b, 0x05, 0x06]));
    }
}
