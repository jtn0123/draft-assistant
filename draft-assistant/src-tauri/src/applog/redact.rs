//! Scrubbing secrets out of anything on its way into the log.
//!
//! The log exists to be pasted into a chat window on draft night, which is
//! exactly the wrong place for an Anthropic key, a companion bearer token or a
//! pairing code. Error strings are the dangerous ones: a failed HTTP call
//! happily quotes back the URL it was made against, query string and all.
//!
//! Deny-list rather than allow-list, because the alternative is logging
//! nothing useful. Everything that has a recognisable shape -- a marker like
//! `token=`, a `Bearer` prefix, an `sk-` key, six digits inside a URL -- is
//! masked; anything else is passed through.
//!
//! Hand-rolled rather than a regex: this crate has no regex dependency and one
//! byte scanner is cheaper than adding one.

/// What replaces a masked value. Recognisable in a log, and not something that
/// can be mistaken for the real thing.
const MASK: &str = "····";

/// Markers whose *value* is a secret. Longest first, so `api_key=` is matched
/// before the `key=` inside it.
const MARKERS: [&str; 11] = [
    "authorization: bearer ",
    "authorization:bearer ",
    "client_secret=",
    "refresh_token=",
    "access_token=",
    "api_key=",
    "apikey=",
    "bearer ",
    "secret=",
    "token=",
    "code=",
];

/// Where a marker's value stops. A query string ends at `&`, a sentence at a
/// space or a comma, a JSON string at a quote.
fn ends_value(c: char) -> bool {
    c.is_whitespace() || matches!(c, '&' | '"' | '\'' | ',' | ')' | ';' | '}' | '#')
}

/// Characters an opaque key or token is made of.
fn key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')
}

/// Mask every secret-shaped run in `input`.
///
/// Never fails and never panics: the worst case is a line that says less than
/// it could have.
pub fn redact(input: &str) -> String {
    // ASCII lowercasing is byte-for-byte, so an index into this is an index
    // into `input`. Matching on it is what makes every marker above
    // case-insensitive without allocating per comparison.
    let lower = input.to_ascii_lowercase();
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    let mut low = lower.as_str();
    // Whether the scanner is part-way through a URL, which is the only place a
    // bare six-digit run is assumed to be a pairing code rather than a score.
    let mut in_url = false;

    while !rest.is_empty() {
        if let Some(marker) = MARKERS.iter().find(|m| low.starts_with(**m)) {
            out.push_str(&rest[..marker.len()]);
            let value_len = rest[marker.len()..]
                .find(ends_value)
                .unwrap_or(rest.len() - marker.len());
            if value_len > 0 {
                out.push_str(MASK);
            }
            let step = marker.len() + value_len;
            rest = &rest[step..];
            low = &low[step..];
            continue;
        }
        if low.starts_with("sk-") {
            let len = rest.find(|c| !key_char(c)).unwrap_or(rest.len());
            // `sk-` on its own is a word, not a key; a key has a body.
            if len > 8 {
                out.push_str("sk-");
                out.push_str(MASK);
                rest = &rest[len..];
                low = &low[len..];
                continue;
            }
        }
        if low.starts_with("://") {
            in_url = true;
        }
        let c = rest.chars().next().unwrap_or(' ');
        if in_url && c.is_ascii_digit() {
            let len = rest
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len());
            // Exactly six: a league id is eighteen digits and a port is four,
            // and neither is worth hiding.
            out.push_str(if len == 6 { MASK } else { &rest[..len] });
            rest = &rest[len..];
            low = &low[len..];
            continue;
        }
        if in_url && c.is_whitespace() {
            in_url = false;
        }
        let step = c.len_utf8();
        out.push(c);
        rest = &rest[step..];
        low = &low[step..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_anthropic_key_quoted_back_in_an_error_is_masked() {
        let masked = redact("auth failed for sk-ant-api03-AbC123defGHI456jkl and retried");
        assert_eq!(masked, "auth failed for sk-···· and retried");
        assert!(!masked.contains("api03"));
    }

    #[test]
    fn a_bearer_token_in_a_header_dump_is_masked() {
        assert_eq!(
            redact("authorization: Bearer eyJhbGciOi.J9.xyz"),
            "authorization: Bearer ····",
        );
        assert_eq!(
            redact("sent Bearer abc123 to the host"),
            "sent Bearer ···· to the host"
        );
    }

    #[test]
    fn a_six_digit_pairing_code_in_a_url_is_masked_but_a_league_id_is_not() {
        assert_eq!(
            redact("GET http://192.168.1.24:7878/pair/418902 failed"),
            "GET http://192.168.1.24:7878/pair/···· failed",
        );
        // Eighteen digits: a Sleeper league id, and the whole point of the
        // line. Masking it would make the log useless.
        assert_eq!(
            redact("https://api.sleeper.app/v1/league/123456789012345678"),
            "https://api.sleeper.app/v1/league/123456789012345678",
        );
    }

    #[test]
    fn six_digits_outside_a_url_are_left_alone() {
        // A pick number, a timestamp, a point total: none of them secret.
        assert_eq!(redact("tick 123456 finished"), "tick 123456 finished");
    }

    #[test]
    fn the_value_of_every_secret_marker_is_masked_and_the_name_survives() {
        assert_eq!(redact("?code=418902&state=x"), "?code=····&state=x");
        assert_eq!(redact("client_secret=abc123"), "client_secret=····");
        assert_eq!(redact("token=deadbeef end"), "token=···· end");
        // The longer marker wins over the `key=` inside it, so the mask is not
        // applied twice and `api_` is not left dangling.
        assert_eq!(redact("api_key=abc123"), "api_key=····");
    }

    #[test]
    fn a_marker_with_nothing_after_it_is_left_as_it_is() {
        assert_eq!(redact("token="), "token=");
        assert_eq!(redact("code=&next"), "code=&next");
    }

    #[test]
    fn ordinary_text_is_returned_unchanged() {
        let line = "the projection source did not answer in 10s (league Dynasty Warriors)";
        assert_eq!(redact(line), line);
        assert_eq!(redact(""), "");
    }

    #[test]
    fn non_ascii_text_survives_the_scan() {
        // The scanner indexes an ASCII-lowercased copy; a multi-byte character
        // must not shift those indices or split a character in half.
        assert_eq!(
            redact("Renée · token=abc · done"),
            "Renée · token=···· · done"
        );
    }
}
