//! `Base64Coder`.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `encodeString`, for bytes.
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let group = chunk.iter().fold(0u32, |sum, byte| (sum << 8) | u32::from(*byte)) << (8 * (3 - chunk.len()));
        for index in 0..4 {
            out.push(if index <= chunk.len() { char::from(ALPHABET[(group >> (18 - 6 * index)) as usize & 63]) } else { '=' });
        }
    }
    out
}

/// `decode`: `None` for what isn't Base64.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut bits, mut count) = (0u32, 0);
    for byte in text.bytes().filter(|byte| *byte != b'=') {
        bits = (bits << 6) | ALPHABET.iter().position(|known| *known == byte)? as u32;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
        }
    }
    Some(out)
}

/// `encodeLines`: 76 characters to a line, each line ended.
pub fn encode_lines(bytes: &[u8]) -> String {
    let mut out = String::new();
    for line in bytes.chunks(57) {
        out.push_str(&encode(line));
        out.push('\n');
    }
    out
}

/// `decodeLines`: the same, past any white space.
pub fn decode_lines(text: &str) -> Option<Vec<u8>> {
    decode(&text.split_whitespace().collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_values() {
        assert_eq!(encode(b"Man"), "TWFu");
        assert_eq!(encode(b"Ma"), "TWE=");
        assert_eq!(encode(b"M"), "TQ==");
        assert_eq!(decode("TWE=").as_deref(), Some(&b"Ma"[..]));
        assert_eq!(decode("not base64!"), None);
    }

    #[test]
    fn lines_wrap_and_come_back() {
        let bytes: Vec<u8> = (0..=255).cycle().take(1000).collect();
        let text = encode_lines(&bytes);
        assert!(text.lines().all(|line| line.len() <= 76));
        assert!(text.ends_with('\n'));
        assert_eq!(decode_lines(&text), Some(bytes));
    }
}
