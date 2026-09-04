//! Turning the data URLs the image cache hands back into bytes on the wire.
//!
//! `Engine::headshot` and `Engine::avatar` answer with `data:<mime>;base64,…`,
//! which is what the webview wants and what the disk cache holds. The phone
//! asks for a plain image over HTTP, so the URL is taken apart here rather
//! than a second copy of the cache being kept in another shape. The decoder is
//! twenty lines; no base64 crate is added to the dependency graph for it.

/// The MIME type and the raw bytes of a `data:` URL, or nothing if it is not
/// one this app produced.
pub fn decode_data_url(url: &str) -> Option<(String, Vec<u8>)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, payload) = rest.split_once(',')?;
    let mime = meta.strip_suffix(";base64")?;
    if mime.is_empty() || !mime.starts_with("image/") {
        return None;
    }
    Some((mime.to_string(), decode_base64(payload)?))
}

/// Standard base64, no line breaks, optional `=` padding.
fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for byte in input.bytes() {
        if byte == b'=' {
            break;
        }
        let six = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            // A stray newline in a wrapped data URL is not a reason to fail.
            b'\r' | b'\n' => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(six);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{decode_base64, decode_data_url};

    #[test]
    fn a_png_data_url_comes_apart_into_a_type_and_bytes() {
        // "hello" in base64 is aGVsbG8=
        let (mime, bytes) = decode_data_url("data:image/png;base64,aGVsbG8=").expect("decodes");
        assert_eq!(mime, "image/png");
        assert_eq!(bytes, b"hello");
    }

    #[test]
    fn every_length_of_payload_round_trips() {
        // The three tail cases of base64: no padding, one =, two ==.
        for (encoded, expected) in [
            ("YWJj", "abc"),
            ("YWJjZA==", "abcd"),
            ("YWJjZGU=", "abcde"),
            ("", ""),
        ] {
            assert_eq!(
                decode_base64(encoded).expect("decodes"),
                expected.as_bytes(),
                "{encoded}"
            );
        }
    }

    #[test]
    fn anything_that_is_not_an_image_data_url_is_refused() {
        assert!(decode_data_url("https://example.com/a.png").is_none());
        assert!(decode_data_url("data:image/png,notbase64").is_none());
        assert!(decode_data_url("data:;base64,aGVsbG8=").is_none());
        // Refusing non-image types is what stops this route ever becoming a
        // way to serve an arbitrary blob out of the cache.
        assert!(decode_data_url("data:text/html;base64,aGVsbG8=").is_none());
        assert!(decode_data_url("data:image/png;base64,not valid!").is_none());
    }
}
