//! Modifiziertes UTF-7 für IMAP-Ordnernamen (RFC 3501, 5.1.3).
//!
//! Beispiel: `Entw&APw-rfe` → `Entwürfe`. Eigene Mini-Implementierung statt
//! einer weiteren Abhängigkeit.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+,";

fn sextet(c: u8) -> Option<u32> {
    ALPHABET
        .iter()
        .position(|&a| a == c)
        .and_then(|p| u32::try_from(p).ok())
}

/// Dekodiert einen Ordnernamen. Ungültige Sequenzen bleiben unverändert stehen.
pub fn decode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('-') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let chunk = &after[..end];
        if chunk.is_empty() {
            out.push('&');
        } else if let Some(decoded) = decode_chunk(chunk) {
            out.push_str(&decoded);
        } else {
            out.push_str(&rest[start..start + 1 + end + 1]);
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn decode_chunk(chunk: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(chunk.len() * 3 / 4);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for c in chunk.bytes() {
        buffer = (buffer << 6) | sextet(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push(u8::try_from((buffer >> bits) & 0xFF).ok()?);
        }
    }
    let (pairs, _) = bytes.as_chunks::<2>();
    let units: Vec<u16> = pairs.iter().map(|p| u16::from_be_bytes(*p)).collect();
    String::from_utf16(&units).ok()
}

/// Kodiert einen Ordnernamen (für Ordner anlegen/umbenennen ab M4).
pub fn encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pending: Vec<u16> = Vec::new();
    let flush = |pending: &mut Vec<u16>, out: &mut String| {
        if pending.is_empty() {
            return;
        }
        out.push('&');
        let bytes: Vec<u8> = pending.iter().flat_map(|u| u.to_be_bytes()).collect();
        let mut buffer: u32 = 0;
        let mut bits = 0;
        for b in bytes {
            buffer = (buffer << 8) | u32::from(b);
            bits += 8;
            while bits >= 6 {
                bits -= 6;
                out.push(char::from(ALPHABET[((buffer >> bits) & 0x3F) as usize]));
            }
        }
        if bits > 0 {
            out.push(char::from(
                ALPHABET[((buffer << (6 - bits)) & 0x3F) as usize],
            ));
        }
        out.push('-');
        pending.clear();
    };
    for ch in input.chars() {
        if ch == '&' {
            flush(&mut pending, &mut out);
            out.push_str("&-");
        } else if (' '..='~').contains(&ch) {
            flush(&mut pending, &mut out);
            out.push(ch);
        } else {
            let mut buf = [0u16; 2];
            pending.extend_from_slice(ch.encode_utf16(&mut buf));
        }
    }
    flush(&mut pending, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_examples() {
        assert_eq!(decode("INBOX"), "INBOX");
        assert_eq!(decode("Entw&APw-rfe"), "Entwürfe");
        assert_eq!(decode("Gel&APY-scht"), "Gelöscht");
        assert_eq!(decode("Tom &- Jerry"), "Tom & Jerry");
        assert_eq!(decode("&ZeVnLIqe-"), "日本語");
    }

    #[test]
    fn invalid_input_is_kept() {
        assert_eq!(decode("kaputt&"), "kaputt&");
        assert_eq!(decode("a&!!-b"), "a&!!-b");
    }

    #[test]
    fn encode_roundtrip() {
        for s in [
            "INBOX",
            "Entwürfe",
            "Tom & Jerry",
            "日本語",
            "Emoji 🐦 Ordner",
        ] {
            assert_eq!(decode(&encode(s)), s, "{s}");
        }
        assert_eq!(encode("Entwürfe"), "Entw&APw-rfe");
    }
}
