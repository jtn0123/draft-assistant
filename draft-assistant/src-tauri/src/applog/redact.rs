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
const MARKERS: [&str; 13] = [
    "authorization: bearer ",
    "authorization:bearer ",
    "authorization: basic ",
    "authorization:basic ",
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

/// JSON keys whose string value is a secret: the shape a token response or a
/// serialised config takes, `"access_token": "abc"`, which none of the `=`
/// markers above ever matched.
const JSON_KEYS: [&str; 5] = [
    "access_token",
    "refresh_token",
    "client_secret",
    "api_key",
    "token",
];

/// The length of a `"key": "` prefix opening `low`, when `low` starts with one
/// of the JSON secret keys and a quoted value. Whitespace is allowed around
/// the colon, as pretty-printing puts it.
fn json_secret_prefix(low: &str) -> Option<usize> {
    let rest = low.strip_prefix('"')?;
    let key = JSON_KEYS.iter().find(|key| rest.starts_with(**key))?;
    let rest = rest[key.len()..].strip_prefix('"')?;
    let rest = rest.trim_start_matches([' ', '\t']).strip_prefix(':')?;
    let rest = rest.trim_start_matches([' ', '\t']).strip_prefix('"')?;
    Some(low.len() - rest.len())
}

/// Where a marker's value stops. A query string ends at `&`, a sentence at a
/// space or a comma, a JSON string at a quote.
fn ends_value(c: char) -> bool {
    c.is_whitespace() || matches!(c, '&' | '"' | '\'' | ',' | ')' | ';' | '}' | '#')
}

/// Characters an opaque key or token is made of.
fn key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')
}

/// Characters a base64 credential is made of.
fn base64_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=')
}

/// The length of the `Basic ` credential opening `low`, when there is one,
/// or `None` when the word is plain English ("a basic thing").
///
/// A bare `Basic` cannot be a marker the way `Bearer` is: it is an ordinary
/// word. So what follows has to look like base64 of `user:password`: eight
/// or more base64 characters with a digit, a capital after the first letter,
/// or padding somewhere in them. `basic understanding` has none of those;
/// `Basic dXNlcjpwYXNz` has a capital in the middle.
fn basic_credential_len(low: &str, rest: &str) -> Option<usize> {
    const WORD: &str = "basic ";
    if !low.starts_with(WORD) {
        return None;
    }
    let value = &rest[WORD.len()..];
    let len = value.find(|c| !base64_char(c)).unwrap_or(value.len());
    let value = &value[..len];
    if len < 8 {
        return None;
    }
    let looks_encoded = value
        .chars()
        .any(|c| c.is_ascii_digit() || matches!(c, '+' | '/' | '='))
        || value.chars().skip(1).any(|c| c.is_ascii_uppercase());
    looks_encoded.then_some(WORD.len() + len)
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
        if let Some(prefix) = json_secret_prefix(low) {
            out.push_str(&rest[..prefix]);
            let value_len = rest[prefix..].find('"').unwrap_or(rest.len() - prefix);
            if value_len > 0 {
                out.push_str(MASK);
            }
            let step = prefix + value_len;
            rest = &rest[step..];
            low = &low[step..];
            continue;
        }
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
        if let Some(len) = basic_credential_len(low, rest) {
            out.push_str(&rest[.."basic ".len()]);
            out.push_str(MASK);
            rest = &rest[len..];
            low = &low[len..];
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
        // A pairing code sits in its own path segment or query value, so the
        // run has to follow a `/` or an `=`. A Yahoo league key is `449.l.123456`:
        // six digits after a dot, and the whole point of the line it is on.
        let after_separator = matches!(out.chars().next_back(), Some('/' | '='));
        if in_url && c.is_ascii_digit() && after_separator {
            let len = rest
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len());
            // Exactly six: a Sleeper league id is eighteen digits and a port
            // is four, and neither is worth hiding.
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

    /// The failure this prevents: `Bearer` was masked and `Basic` was not,
    /// so a proxy or a projection source behind HTTP basic auth quoted
    /// `user:password`, base64 and all, straight into the log.
    #[test]
    fn a_basic_credential_is_masked_in_a_header_and_in_a_sentence() {
        assert_eq!(
            redact("Authorization: Basic YWRtaW46aHVudGVyMg=="),
            "Authorization: Basic ····",
        );
        assert_eq!(
            redact("proxy sent authorization:Basic dXNlcjpwYXNz and got 407"),
            "proxy sent authorization:Basic ···· and got 407",
        );
        // No header word, but the shape is unmistakable.
        assert_eq!(
            redact("retried with Basic dXNlcjpwYXNz on the second try"),
            "retried with Basic ···· on the second try",
        );
    }

    #[test]
    fn the_english_word_basic_is_left_alone() {
        for line in [
            "a basic thing went wrong",
            "basic understanding of the board",
            "Basic Authentication was refused",
            "the basic 2 step flow",
        ] {
            assert_eq!(redact(line), line);
        }
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

    /// The failure this prevents: every Yahoo request line in the log read
    /// `league/449.l.····`, so the one id that said which league had failed
    /// was the one thing the log would not say.
    #[test]
    fn a_yahoo_league_key_in_a_url_survives_because_it_does_not_follow_a_slash() {
        let line = "GET https://fantasysports.yahooapis.com/fantasy/v2/league/449.l.123456/draftresults failed";
        assert_eq!(redact(line), line);
        let key_only =
            "https://fantasysports.yahooapis.com/fantasy/v2/league/nfl.l.654321;out=settings";
        assert_eq!(redact(key_only), key_only);
    }

    #[test]
    fn a_six_digit_query_value_in_a_url_is_still_masked() {
        assert_eq!(
            redact("GET http://192.168.1.24:7878/pair?c=418902 refused"),
            "GET http://192.168.1.24:7878/pair?c=···· refused",
        );
    }

    /// A token response or a serialised config quotes its secrets as JSON,
    /// and no `=` marker ever matched `"access_token": "…"`.
    #[test]
    fn a_secret_in_a_json_object_is_masked_under_every_key_it_is_stored_as() {
        assert_eq!(
            redact(r#"{"access_token": "ya29.abc", "expires_in": 3600}"#),
            r#"{"access_token": "····", "expires_in": 3600}"#,
        );
        assert_eq!(
            redact(r#"{"refresh_token":"r1//xyz"}"#),
            r#"{"refresh_token":"····"}"#
        );
        assert_eq!(redact(r#""token": "deadbeef""#), r#""token": "····""#);
        assert_eq!(
            redact(r#"{"api_key" : "sk-ant-api03-AbCdEfGhIjKl"}"#),
            r#"{"api_key" : "····"}"#
        );
        assert_eq!(
            redact(r#"{"client_secret": "hunter2", "client_id": "abc"}"#),
            r#"{"client_secret": "····", "client_id": "abc"}"#
        );
    }

    #[test]
    fn a_json_key_that_merely_starts_like_a_secret_is_left_alone() {
        // `tokens` is a count, not a credential, and an unquoted value is a
        // number: neither is the shape being hunted.
        let line = r#"{"tokens": "512", "token_count": 3, "code": 404}"#;
        assert_eq!(redact(line), line);
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
