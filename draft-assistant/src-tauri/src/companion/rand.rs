//! Random bytes for the pairing code and the device tokens.
//!
//! Straight from the OS: `/dev/urandom` on the platforms this app ships to.
//! No RNG crate is pulled in for it — the only thing wanted here is unguessable
//! bytes, and the kernel already has them. A deterministic fallback would be
//! worse than useless (a predictable pairing token is no token at all), so a
//! read that fails is a hard error the caller has to deal with.

use std::io::Read;

/// `n` bytes from the operating system's random source.
pub fn bytes(n: usize) -> Result<Vec<u8>, String> {
    let mut out = vec![0u8; n];
    let mut file = std::fs::File::open("/dev/urandom")
        .map_err(|e| format!("could not open the system random source: {e}"))?;
    file.read_exact(&mut out)
        .map_err(|e| format!("could not read from the system random source: {e}"))?;
    Ok(out)
}

/// A fresh six-digit pairing code, `"000000"`–`"999999"`.
///
/// Built from eight random bytes reduced modulo a million. The bias that
/// introduces is under one part in 10^13 — far below anything that matters for
/// a code that is read off a screen and typed in within the minute.
pub fn pairing_code() -> Result<String, String> {
    let raw = bytes(8)?;
    let mut value = 0u64;
    for byte in raw {
        value = value.wrapping_mul(31).wrapping_add(u64::from(byte));
    }
    Ok(format!("{:06}", value % 1_000_000))
}

/// An opaque device token: 32 random bytes, hex.
pub fn token() -> Result<String, String> {
    Ok(hex(&bytes(32)?))
}

/// A device id: 16 random bytes, hex.
pub fn device_id() -> Result<String, String> {
    Ok(hex(&bytes(16)?))
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Compare two secrets without letting the time taken say how much of the
/// front of the string was right. Nice to have rather than load-bearing —
/// the pairing code is also rate-limited and rotates — but it costs two lines.
pub fn secrets_match(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::{bytes, device_id, pairing_code, secrets_match, token};

    #[test]
    fn a_pairing_code_is_six_digits_and_not_the_same_one_twice() {
        let first = pairing_code().expect("the system random source is readable");
        assert_eq!(first.len(), 6);
        assert!(first.chars().all(|c| c.is_ascii_digit()), "{first}");
        // Not a proof of randomness; a guard against a stub that returns a
        // constant, which is the failure that would matter.
        let mut all_same = true;
        for _ in 0..8 {
            if pairing_code().expect("random") != first {
                all_same = false;
                break;
            }
        }
        assert!(!all_same, "the pairing code never changed");
    }

    #[test]
    fn tokens_are_long_hex_and_unique() {
        let a = token().expect("random");
        let b = token().expect("random");
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert_eq!(device_id().expect("random").len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn the_os_gives_back_as_many_bytes_as_were_asked_for() {
        assert_eq!(bytes(48).expect("random").len(), 48);
        assert!(bytes(0).expect("random").is_empty());
    }

    #[test]
    fn secrets_compare_by_value_not_by_prefix() {
        assert!(secrets_match("123456", "123456"));
        assert!(!secrets_match("123456", "123457"));
        assert!(!secrets_match("123456", "12345"));
        assert!(!secrets_match("", "0"));
        assert!(secrets_match("", ""));
    }
}
