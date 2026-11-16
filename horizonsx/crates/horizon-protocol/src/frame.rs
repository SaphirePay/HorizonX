//! Packet framing (Varint21FrameDecoder / Varint21LengthFieldPrepender) and
//! zlib compression above a threshold (CompressionEncoder / CompressionDecoder).
use crate::{
    Error, Result,
    buf::{Reader, write_varint},
};
use flate2::{Compression, read::ZlibDecoder};
use std::io::Read;

/// Frames are at most three VarInt bytes long (21 bits).
pub const MAX_FRAME: usize = (1 << 21) - 1;
/// CompressionDecoder.MAXIMUM_UNCOMPRESSED_LENGTH.
pub const MAX_UNCOMPRESSED: usize = 8_388_608;

/// Build the frame for one packet body (`id` + payload already concatenated):
/// length prefix, and with compression enabled the uncompressed-length
/// field (0 when below the threshold).
pub fn encode(body: &[u8], threshold: Option<usize>) -> Result<Vec<u8>> {
    Ok(prefixed(&contents(body, threshold)?))
}

/// What [`encode`] puts after the length prefix: the same for every
/// connection with the same threshold.
pub fn contents(body: &[u8], threshold: Option<usize>) -> Result<Vec<u8>> {
    let inner = match threshold {
        None => body.to_vec(),
        Some(t) if body.len() < t => {
            let mut v = Vec::with_capacity(body.len() + 1);
            v.push(0);
            v.extend_from_slice(body);
            v
        }
        Some(_) => {
            let mut v = Vec::with_capacity(body.len() / 2 + 64);
            write_varint(&mut v, body.len() as i32);
            zlib_into(&mut v, body)?;
            v
        }
    };
    if inner.len() > MAX_FRAME {
        return Err(Error::new(format!(
            "packet too large: {} bytes",
            inner.len()
        )));
    }
    Ok(inner)
}

/// zlib compressors, reset for each packet: a new one is a few hundred KB of state allocated and freed per
/// packet, thousands of times a second on a busy server. A reset stream writes the same bytes a new one
/// does. HorizonsX: one shared pool, of which at most `KEPT_DEFLATE` stay between packets, where one per
/// thread kept a compressor on every connection's writer thread (36 MB at 50 players).
/// The zlib level packets are compressed at (`HORIZON_ZLIB_LEVEL`, 0 to 9, default 2, Hyperion's). Measured with 600
/// spread players: the connections' writer threads 1.85 cores at 6, 1.44 at 2, for 8.6% more bytes a chunk.
fn zlib_level() -> Compression {
    static LEVEL: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    Compression::new(*LEVEL.get_or_init(|| {
        std::env::var("HORIZON_ZLIB_LEVEL").ok().and_then(|v| v.trim().parse().ok()).map_or(2, |v: u32| v.min(9))
    }))
}

static DEFLATE: std::sync::Mutex<Vec<flate2::Compress>> = std::sync::Mutex::new(Vec::new());
const KEPT_DEFLATE: usize = 16;

/// `body` as one zlib stream at the default level, appended to `out`.
fn zlib_into(out: &mut Vec<u8>, body: &[u8]) -> Result<()> {
    let taken = DEFLATE.lock().unwrap_or_else(|e| e.into_inner()).pop();
    let mut z = taken.unwrap_or_else(|| flate2::Compress::new(zlib_level(), true));
    z.reset();
    let result = loop {
        let used = z.total_in() as usize;
        if out.capacity() - out.len() < 64 {
            out.reserve(out.capacity().max(256));
        }
        match z.compress_vec(&body[used..], out, flate2::FlushCompress::Finish) {
            Ok(flate2::Status::StreamEnd) => break Ok(()),
            Ok(_) => {}
            Err(e) => break Err(Error::new(format!("zlib: {e}"))),
        }
    };
    let mut pool = DEFLATE.lock().unwrap_or_else(|e| e.into_inner());
    if pool.len() < KEPT_DEFLATE {
        pool.push(z);
    }
    result
}

/// A frame: the length prefix, then the [`contents`].
pub fn prefixed(contents: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(contents.len() + 3);
    write_varint(&mut out, contents.len() as i32);
    out.extend_from_slice(contents);
    out
}

/// Split one complete frame off the front of `buffer`, if available.
/// Returns the frame contents (without the length prefix).
pub fn split(buffer: &mut Vec<u8>) -> Result<Option<Vec<u8>>> {
    let mut len = 0usize;
    for i in 0..3 {
        let Some(&b) = buffer.get(i) else {
            return Ok(None);
        };
        len |= usize::from(b & 0x7F) << (7 * i);
        if b & 0x80 == 0 {
            if len == 0 {
                // Varint21FrameDecoder: a frame with no bytes is a
                // corrupted frame, not an empty packet.
                return Err(Error::new("Frame length cannot be zero"));
            }
            let start = i + 1;
            if buffer.len() < start + len {
                return Ok(None);
            }
            let frame = buffer[start..start + len].to_vec();
            buffer.drain(..start + len);
            return Ok(Some(frame));
        }
    }
    Err(Error::new("length wider than 21 bits"))
}

/// Undo compression on a frame, giving the packet body (id + payload).
pub fn decompress(frame: Vec<u8>, threshold: Option<usize>) -> Result<Vec<u8>> {
    let Some(threshold) = threshold else {
        return Ok(frame);
    };
    let mut r = Reader::new(&frame);
    let size = r.varint()?;
    if size == 0 {
        return Ok(frame[r.position()..].to_vec());
    }
    let size = usize::try_from(size).map_err(|_| Error::new("negative uncompressed length"))?;
    if size < threshold {
        return Err(Error::new(format!(
            "Badly compressed packet - size of {size} is below server threshold of {threshold}"
        )));
    }
    if size > MAX_UNCOMPRESSED {
        return Err(Error::new(format!(
            "Badly compressed packet - size of {size} is larger than protocol maximum"
        )));
    }
    let start = r.position();
    let mut out = Vec::with_capacity(size);
    ZlibDecoder::new(&frame[start..])
        .take(size as u64 + 1)
        .read_to_end(&mut out)?;
    if out.len() != size {
        return Err(Error::new(format!(
            "Badly compressed packet - actual length {} does not match declared size {size}",
            out.len()
        )));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::ZlibEncoder;
    use std::io::Write;

    #[test]
    fn uncompressed_frames_round_trip() {
        let body = vec![0x00, 1, 2, 3];
        let framed = encode(&body, None).unwrap();
        assert_eq!(framed, [4, 0, 1, 2, 3]);
        let mut buffer = framed.clone();
        buffer.extend_from_slice(&framed[..2]);
        assert_eq!(split(&mut buffer).unwrap(), Some(body));
        assert_eq!(split(&mut buffer).unwrap(), None);
        assert_eq!(buffer, [4, 0]);
    }

    #[test]
    fn compression_threshold_like_vanilla() {
        // Below the threshold: a zero data length, then the raw body.
        let small = vec![7u8; 10];
        let framed = encode(&small, Some(256)).unwrap();
        assert_eq!(&framed[..2], &[11, 0]);
        let mut buffer = framed;
        let frame = split(&mut buffer).unwrap().unwrap();
        assert_eq!(decompress(frame, Some(256)).unwrap(), small);
        // At or above: the uncompressed size and a zlib stream.
        let big: Vec<u8> = (0..5000u32).map(|i| (i % 7) as u8).collect();
        let framed = encode(&big, Some(256)).unwrap();
        assert!(framed.len() < big.len());
        let mut buffer = framed;
        let frame = split(&mut buffer).unwrap().unwrap();
        assert_eq!(Reader::new(&frame).varint().unwrap(), 5000);
        assert_eq!(decompress(frame, Some(256)).unwrap(), big);
    }

    #[test]
    fn the_reused_compressor_writes_what_a_new_one_does() {
        for size in [300usize, 5000, 70_000, 300_000] {
            let body: Vec<u8> = (0..size as u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
            let mut fresh = Vec::new();
            write_varint(&mut fresh, body.len() as i32);
            let mut z = ZlibEncoder::new(fresh, zlib_level());
            z.write_all(&body).unwrap();
            let fresh = z.finish().unwrap();
            // Twice: the second reuses the thread's compressor after a reset.
            assert_eq!(contents(&body, Some(256)).unwrap(), fresh);
            assert_eq!(contents(&body, Some(256)).unwrap(), fresh);
        }
    }

    #[test]
    fn rejects_bad_compressed_lengths() {
        // Declared size below the threshold is a protocol error.
        let mut frame = vec![10];
        let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
        z.write_all(&[1; 10]).unwrap();
        frame.extend(z.finish().unwrap());
        assert!(decompress(frame, Some(256)).is_err());
        // Declared size that does not match the stream.
        let mut frame = Vec::new();
        write_varint(&mut frame, 300);
        let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
        z.write_all(&[1; 299]).unwrap();
        frame.extend(z.finish().unwrap());
        assert!(decompress(frame, Some(256)).is_err());
    }

    #[test]
    fn a_noncanonical_length_prefix_still_reads() {
        // The data-length VarInt is read, not measured: zero as `0x80 0x00`.
        assert_eq!(
            decompress(vec![0x80, 0x00, 7, 1, 2], Some(256)).unwrap(),
            [7, 1, 2]
        );
        // And a compressed packet whose size was written wide: 300.
        let body = vec![9; 300];
        let mut frame = vec![0xAC, 0x82, 0x00];
        let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
        z.write_all(&body).unwrap();
        frame.extend(z.finish().unwrap());
        assert_eq!(decompress(frame, Some(256)).unwrap(), body);
    }
}
