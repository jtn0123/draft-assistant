//! What a non-2xx status from the Messages API means, and which ones are
//! worth asking again about.
//!
//! A 429, a 529 and any 5xx are the API saying "not right now" rather than
//! "not that": the same request a second later usually goes through. Each
//! used to reach the panel on the first try as a dead end, and a 529 read as
//! "unknown status code" because the HTTP library has no name for it.

use serde::Deserialize;
use std::time::Duration;

/// How many times one question is sent before its status is given up on.
pub(super) const MAX_ATTEMPTS: u32 = 3;

/// The longest a `retry-after` is honoured for. Past this the user is better
/// served by the error and a choice than by a panel that sits on "Thinking".
const MAX_RETRY_AFTER: Duration = Duration::from_secs(30);

/// The pause before the first retry when the API names none. Doubles each
/// time. Only [`super::ask`] uses this value; the wire tests pass their own.
pub(super) const BASE_BACKOFF: Duration = Duration::from_secs(1);

#[derive(Deserialize)]
struct ApiErrorBody {
    #[serde(default)]
    error: Option<ApiErrorDetail>,
}

#[derive(Deserialize)]
struct ApiErrorDetail {
    #[serde(default)]
    message: Option<String>,
}

/// Whether a status is one the same request may get past on a second try.
pub(super) fn is_retryable(status: reqwest::StatusCode) -> bool {
    status.as_u16() == 429 || status.as_u16() == 529 || status.is_server_error()
}

/// The `retry-after` header as a pause, when the API sent one in seconds. An
/// HTTP-date form is read past: the backoff stands in for it.
pub(super) fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let seconds: f64 = headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    Some(Duration::from_secs_f64(seconds).min(MAX_RETRY_AFTER))
}

/// How long to wait before attempt `next` (counted from 1), or `None` when
/// the status is not worth retrying or the attempts are used up. The API's
/// own `retry-after` wins over the backoff when it names one.
pub(super) fn delay_before(
    status: reqwest::StatusCode,
    next: u32,
    retry_after: Option<Duration>,
    base: Duration,
) -> Option<Duration> {
    if !is_retryable(status) || next > MAX_ATTEMPTS {
        return None;
    }
    Some(retry_after.unwrap_or_else(|| base * 2u32.pow(next.saturating_sub(2))))
}

/// The sentence the panel shows for a non-2xx status.
pub(super) fn error_message(status: reqwest::StatusCode, body: &str) -> String {
    let detail = serde_json::from_str::<ApiErrorBody>(body)
        .ok()
        .and_then(|b| b.error.and_then(|e| e.message));
    if detail.is_none() {
        // Anything between here and Anthropic can answer with its own error
        // page. Pasting a gateway's HTML into the chat panel tells the user
        // nothing and looks like the model said it, so the status speaks for
        // itself and a short, redacted sample goes to the log. Not the raw
        // body: a proxy's page can echo the request back, headers and all.
        let sample: String = body
            .chars()
            .take(120)
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        crate::applog::warn(format!(
            "Anthropic API {} with a body that is not an error object ({} bytes): {}",
            status.as_u16(),
            body.len(),
            crate::applog::redact(&sample)
        ));
    }
    let sentence = match status.as_u16() {
        401 => "Anthropic rejected the API key".to_string(),
        429 => "Rate limited by Anthropic".to_string(),
        // The HTTP library has no name for 529, so `{status}` printed
        // "529 <unknown status code>" where the one sentence that matters
        // should have been.
        529 => "Anthropic is overloaded, try again in a moment".to_string(),
        code => match status.canonical_reason() {
            Some(reason) => format!("Anthropic API error {code} {reason}"),
            None => format!("Anthropic API error {code}"),
        },
    };
    match detail {
        Some(detail) => format!("{sentence}: {detail}"),
        None => sentence,
    }
}

/// The message once every attempt has come back the same way. It says how
/// many were made, so a rate limit that is still there after three tries reads
/// as one that was waited on rather than one that was hit once.
pub(super) fn gave_up(message: String, attempts: u32) -> String {
    if attempts <= 1 {
        message
    } else {
        format!("{message} (gave up after {attempts} attempts)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    fn status(code: u16) -> StatusCode {
        StatusCode::from_u16(code).expect("a status code")
    }

    #[test]
    fn rate_limits_overload_and_server_errors_are_retried_and_nothing_else_is() {
        for code in [429, 500, 502, 503, 529] {
            assert!(is_retryable(status(code)), "{code}");
        }
        for code in [400, 401, 403, 404, 413] {
            assert!(!is_retryable(status(code)), "{code}");
        }
    }

    #[test]
    fn the_backoff_doubles_and_stops_after_the_last_attempt() {
        let base = Duration::from_millis(100);
        assert_eq!(
            delay_before(status(429), 2, None, base),
            Some(Duration::from_millis(100))
        );
        assert_eq!(
            delay_before(status(503), 3, None, base),
            Some(Duration::from_millis(200))
        );
        assert_eq!(delay_before(status(503), 4, None, base), None);
        assert_eq!(delay_before(status(401), 2, None, base), None);
    }

    #[test]
    fn retry_after_wins_over_the_backoff_and_is_capped() {
        let base = Duration::from_millis(100);
        let named = Some(Duration::from_secs(7));
        assert_eq!(delay_before(status(429), 2, named, base), named);

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::RETRY_AFTER, "2".parse().unwrap());
        assert_eq!(retry_after(&headers), Some(Duration::from_secs(2)));
        headers.insert(reqwest::header::RETRY_AFTER, "600".parse().unwrap());
        assert_eq!(retry_after(&headers), Some(MAX_RETRY_AFTER));
        // The HTTP-date form, and nonsense, fall back to the backoff.
        headers.insert(
            reqwest::header::RETRY_AFTER,
            "Wed, 21 Oct 2026 07:28:00 GMT".parse().unwrap(),
        );
        assert_eq!(retry_after(&headers), None);
        headers.insert(reqwest::header::RETRY_AFTER, "-3".parse().unwrap());
        assert_eq!(retry_after(&headers), None);
        assert_eq!(retry_after(&reqwest::header::HeaderMap::new()), None);
    }

    #[test]
    fn a_529_is_named_as_overload_rather_than_an_unknown_status() {
        let message = error_message(status(529), r#"{"error":{"message":"Overloaded"}}"#);
        assert_eq!(
            message,
            "Anthropic is overloaded, try again in a moment: Overloaded"
        );
        assert!(!error_message(status(529), "").contains("unknown"));
        // A code the library does know keeps its name; one it does not is
        // still a number rather than a placeholder.
        assert_eq!(
            error_message(status(503), ""),
            "Anthropic API error 503 Service Unavailable"
        );
        assert_eq!(error_message(status(599), ""), "Anthropic API error 599");
    }

    #[test]
    fn the_final_message_says_how_many_times_it_was_tried() {
        assert_eq!(
            gave_up("Rate limited by Anthropic".into(), 1),
            "Rate limited by Anthropic"
        );
        assert_eq!(
            gave_up("Rate limited by Anthropic".into(), 3),
            "Rate limited by Anthropic (gave up after 3 attempts)"
        );
    }
}
