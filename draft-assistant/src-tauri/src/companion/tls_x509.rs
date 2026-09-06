//! Reading the expiry out of a certificate, and nothing else.
//!
//! The companion has to know whether the certificate on disk is about to run
//! out so it can ask `tailscale cert` for a new one in time. A full X.509
//! parser is a lot of code for one date, and the crates that do it are not in
//! this app's tree, so this walks just far enough into the DER to reach the
//! `Validity` sequence: it is the fifth element of the certificate body, at
//! a fixed position the standard has not moved since 1988.

use tokio_rustls::rustls::pki_types::{pem::PemObject, CertificateDer};

/// One DER element: its tag, its contents, and whatever follows it.
struct Tlv<'a> {
    tag: u8,
    body: &'a [u8],
    rest: &'a [u8],
}

const SEQUENCE: u8 = 0x30;
const INTEGER: u8 = 0x02;
const UTC_TIME: u8 = 0x17;
const GENERALIZED_TIME: u8 = 0x18;
/// `[0] EXPLICIT`, the optional version that comes before the serial number.
const VERSION: u8 = 0xA0;

fn tlv(input: &[u8]) -> Option<Tlv<'_>> {
    let (&tag, after_tag) = input.split_first()?;
    let (&first, after_first) = after_tag.split_first()?;
    let (length, body_start) = if first < 0x80 {
        (usize::from(first), after_first)
    } else {
        // Long form: the low bits say how many length bytes follow. Anything
        // over four is not a certificate anybody serves.
        let count = usize::from(first & 0x7f);
        if count == 0 || count > 4 || after_first.len() < count {
            return None;
        }
        let length = after_first[..count]
            .iter()
            .fold(0usize, |acc, byte| (acc << 8) | usize::from(*byte));
        (length, &after_first[count..])
    };
    if body_start.len() < length {
        return None;
    }
    Some(Tlv {
        tag,
        body: &body_start[..length],
        rest: &body_start[length..],
    })
}

/// The `notAfter` of a DER certificate, as seconds since the Unix epoch.
///
/// `None` for anything that is not laid out as a certificate is: the caller
/// treats an unreadable certificate as one that needs replacing, which is
/// the safe reading of a file nobody can make sense of.
pub fn not_after_unix(der: &[u8]) -> Option<i64> {
    let certificate = tlv(der)?;
    let tbs = tlv(certificate.body)?;
    if certificate.tag != SEQUENCE || tbs.tag != SEQUENCE {
        return None;
    }
    let mut serial = tlv(tbs.body)?;
    if serial.tag == VERSION {
        serial = tlv(serial.rest)?;
    }
    if serial.tag != INTEGER {
        return None;
    }
    let signature = tlv(serial.rest)?;
    let issuer = tlv(signature.rest)?;
    let validity = tlv(issuer.rest)?;
    if validity.tag != SEQUENCE {
        return None;
    }
    let not_before = tlv(validity.body)?;
    let not_after = tlv(not_before.rest)?;
    time_unix(not_after.tag, not_after.body)
}

/// The same, off a PEM file's contents. The first certificate in the file is
/// the leaf, which is the one whose expiry matters.
pub fn not_after_from_pem(pem: &[u8]) -> Option<i64> {
    let leaf = CertificateDer::from_pem_slice(pem).ok()?;
    not_after_unix(&leaf)
}

/// `YYMMDDHHMMSSZ` (UTCTime) or `YYYYMMDDHHMMSSZ` (GeneralizedTime) to Unix
/// seconds. X.509 requires the seconds and the trailing `Z` in both.
fn time_unix(tag: u8, body: &[u8]) -> Option<i64> {
    let text = std::str::from_utf8(body).ok()?;
    let (year, rest) = match tag {
        UTC_TIME => {
            let two_digit: i64 = text.get(0..2)?.parse().ok()?;
            // RFC 5280: 50 to 99 are the 1900s, 00 to 49 the 2000s.
            let year = if two_digit >= 50 {
                1900 + two_digit
            } else {
                2000 + two_digit
            };
            (year, text.get(2..)?)
        }
        GENERALIZED_TIME => (text.get(0..4)?.parse().ok()?, text.get(4..)?),
        _ => return None,
    };
    if rest.len() != 11 || !rest.ends_with('Z') {
        return None;
    }
    let field = |from: usize| rest.get(from..from + 2)?.parse::<i64>().ok();
    let (month, day, hour, minute, second) =
        (field(0)?, field(2)?, field(4)?, field(6)?, field(8)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Days since 1970-01-01 for a proleptic Gregorian date. Howard Hinnant's
/// algorithm; exact for every year a certificate can name.
pub fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted_month = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::{days_from_civil, not_after_unix};

    #[test]
    fn the_calendar_arithmetic_matches_the_epoch_and_a_leap_year() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        // 2026-09-06T00:00:00Z is 1788652800.
        assert_eq!(days_from_civil(2026, 9, 6) * 86_400, 1_788_652_800);
    }

    #[test]
    fn bytes_that_are_not_a_certificate_have_no_expiry() {
        // The failure this prevents: a truncated or corrupt file on disk
        // being read as valid for ever, so the listener served it until a
        // phone refused it.
        assert_eq!(not_after_unix(&[]), None);
        assert_eq!(not_after_unix(&[0x30, 0x03, 0x02, 0x01]), None);
        assert_eq!(not_after_unix(b"-----BEGIN CERTIFICATE-----"), None);
        // A length that claims more bytes than there are.
        assert_eq!(not_after_unix(&[0x30, 0x84, 0xff, 0xff, 0xff, 0xff]), None);
    }
}
