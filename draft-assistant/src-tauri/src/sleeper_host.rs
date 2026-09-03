//! Which host the Sleeper client actually talks to.
//!
//! Every URL in `sleeper.rs` and `season_api.rs` is built against
//! api.sleeper.app. A debug build will send them somewhere else when
//! `DRAFT_ASSISTANT_SLEEPER_BASE` says so — that is how
//! `scripts/replay-sleeper.mjs` stands in for Sleeper and replays a recorded
//! draft's picks on a timer. A release build ignores the variable entirely,
//! so a shipped app can never be pointed at another host.

use std::borrow::Cow;

/// The real API, and the prefix every built URL starts with.
pub(crate) const DEFAULT: &str = "https://api.sleeper.app";

/// Where requests go, decided once for the life of the process.
pub(crate) fn host() -> &'static str {
    static HOST: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HOST.get_or_init(|| {
        let given = if cfg!(debug_assertions) {
            std::env::var("DRAFT_ASSISTANT_SLEEPER_BASE").ok()
        } else {
            None
        };
        parse(given).unwrap_or_else(|| DEFAULT.to_string())
    })
}

/// What the environment variable means, if anything: blank and whitespace are
/// a typo rather than a host, and a trailing slash would double up the one
/// every path already starts with.
fn parse(given: Option<String>) -> Option<String> {
    given
        .map(|base| base.trim().trim_end_matches('/').to_string())
        .filter(|base| !base.is_empty())
}

/// Send one already-built URL to `host`. A no-op — and a borrow, not an
/// allocation — whenever the client is pointed at Sleeper itself, which is
/// every release build and every ordinary run.
///
/// The host is the caller's, not this module's: `SleeperClient` decides it
/// once when it is built, so a test client can be aimed elsewhere without
/// changing anything process-wide.
pub(crate) fn route_to<'a>(url: &'a str, host: &str) -> Cow<'a, str> {
    match url.strip_prefix(DEFAULT) {
        Some(rest) if host != DEFAULT => Cow::Owned(format!("{host}{rest}")),
        _ => Cow::Borrowed(url),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse, route_to, DEFAULT};
    use std::borrow::Cow;

    #[test]
    fn a_replay_host_takes_over_every_sleeper_url_and_nothing_else() {
        let replay = "http://localhost:8787";
        assert_eq!(
            route_to(&format!("{DEFAULT}/v1/league/1"), replay),
            "http://localhost:8787/v1/league/1"
        );
        assert_eq!(
            route_to(&format!("{DEFAULT}/projections/nfl/2026"), replay),
            "http://localhost:8787/projections/nfl/2026"
        );
        // A different host is left alone: only Sleeper is being stood in for.
        let cdn = "https://sleepercdn.com/avatars/abc";
        assert_eq!(route_to(cdn, replay), cdn);
    }

    #[test]
    fn without_an_override_every_url_is_untouched_and_unallocated() {
        let url = format!("{DEFAULT}/v1/league/1");
        let routed = route_to(&url, DEFAULT);
        assert!(matches!(routed, Cow::Borrowed(_)));
        assert_eq!(routed, url);
    }

    #[test]
    fn a_blank_or_slashed_override_is_read_as_the_typo_it_is() {
        assert_eq!(parse(None), None);
        assert_eq!(parse(Some("   ".to_string())), None);
        assert_eq!(parse(Some(String::new())), None);
        assert_eq!(
            parse(Some(" http://localhost:8787/ ".to_string())),
            Some("http://localhost:8787".to_string())
        );
    }
}
