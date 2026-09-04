//! What the "Add a league" box accepts.
//!
//! Four things get pasted into it: a Sleeper league or draft id, a sleeper.com
//! link with one in it, a Yahoo league key (`449.l.12345`), and the address of
//! a Yahoo league page. Telling them apart is pure string work with no network
//! in it, so it lives here on its own and is tested that way; only the last
//! form needs a request afterwards, and `commands_draft` makes it.

use crate::view_types::is_yahoo_key;

/// Pull a Sleeper id out of whatever the user pasted — a bare id, or a URL
/// like `sleeper.com/draft/nfl/1234567890123456789`.
///
/// Anything that is not a run of digits is refused rather than passed through:
/// the result is interpolated straight into a request path, and text that
/// happens to contain `../` would otherwise walk out of `/v1/`.
pub(crate) fn extract_id(input: &str) -> Result<String, String> {
    input
        .split(|c: char| !c.is_ascii_digit())
        .max_by_key(|run| run.len())
        .filter(|run| (15..=25).contains(&run.len()))
        .map(str::to_string)
        .ok_or_else(|| {
            "that doesn't look like a Sleeper ID — paste the league or draft link, \
             or the long number from it"
                .to_string()
        })
}

/// What the paste box turned out to hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Pasted {
    /// A Sleeper league id, or a bare draft id for a mock.
    Sleeper(String),
    /// A whole Yahoo league key (`449.l.12345`): ready to load.
    Yahoo(String),
    /// A Yahoo league URL, which carries the league id but not the game key
    /// that goes in front of it. Resolved against the account's own leagues.
    YahooNumeric(String),
}

/// Read whatever was pasted: a Sleeper id or link, a Yahoo league key, or a
/// Yahoo league URL.
///
/// The Yahoo forms are tried first because neither can be mistaken for a
/// Sleeper id — a key has `.l.` in it and a URL says yahoo.com — and because
/// a Yahoo URL's league id is far too short to survive the Sleeper rule.
pub(crate) fn extract_ref(input: &str) -> Result<Pasted, String> {
    let trimmed = input.trim();
    if is_yahoo_key(trimmed) {
        return Ok(Pasted::Yahoo(trimmed.to_string()));
    }
    if trimmed.to_ascii_lowercase().contains("yahoo.com") {
        return yahoo_url_id(trimmed)
            .map(Pasted::YahooNumeric)
            .ok_or_else(|| {
                "that Yahoo link has no league id in it — open your league and copy the \
             address from the browser, or paste the league key (449.l.12345)"
                    .to_string()
            });
    }
    extract_id(trimmed).map(Pasted::Sleeper)
}

/// The league id out of a Yahoo league URL like
/// `https://football.fantasysports.yahoo.com/f1/12345`.
///
/// The path segment after the game code is the league; a URL that has been
/// clicked into a sub-page (`/f1/12345/2`) carries a team number after it, so
/// it is the *first* number in the path that counts, not the longest.
fn yahoo_url_id(input: &str) -> Option<String> {
    let path = input.split(['?', '#']).next().unwrap_or(input);
    path.split('/')
        .find(|segment| {
            !segment.is_empty()
                && segment.chars().all(|c| c.is_ascii_digit())
                && segment.len() <= 12
        })
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::{extract_id, extract_ref, yahoo_url_id, Pasted};

    #[test]
    fn a_bare_id_and_a_pasted_link_both_work() {
        assert_eq!(
            extract_id("1389710366300200960").unwrap(),
            "1389710366300200960"
        );
        assert_eq!(
            extract_id("https://sleeper.com/draft/nfl/1389710366300200960").unwrap(),
            "1389710366300200960"
        );
        assert_eq!(
            extract_id("  1389710366300200960  ").unwrap(),
            "1389710366300200960"
        );
    }

    #[test]
    fn anything_without_an_id_in_it_is_refused_rather_than_sent_on() {
        // These used to be passed through verbatim and interpolated straight
        // into a request path.
        for junk in [
            "",
            "   ",
            "hello",
            "../../projections/nfl/2025",
            "12345",
            "https://sleeper.com/leagues",
        ] {
            let result = extract_id(junk);
            assert!(
                result.is_err(),
                "{junk:?} should be refused, got {result:?}"
            );
        }
    }

    #[test]
    fn the_error_tells_the_user_what_to_paste() {
        let error = extract_id("nonsense").unwrap_err();
        assert!(error.contains("Sleeper ID"), "unhelpful: {error}");
    }

    #[test]
    fn a_yahoo_league_key_is_taken_whole() {
        assert_eq!(
            extract_ref("449.l.12345").unwrap(),
            Pasted::Yahoo("449.l.12345".into())
        );
        assert_eq!(
            extract_ref("  431.l.987654  ").unwrap(),
            Pasted::Yahoo("431.l.987654".into())
        );
    }

    #[test]
    fn a_yahoo_league_url_gives_up_the_league_id_and_not_the_team_number() {
        for pasted in [
            "https://football.fantasysports.yahoo.com/f1/12345",
            "https://football.fantasysports.yahoo.com/f1/12345/",
            "http://football.fantasysports.yahoo.com/f1/12345/2",
            "https://football.fantasysports.yahoo.com/f1/12345/2/team?week=3",
        ] {
            assert_eq!(
                extract_ref(pasted).unwrap(),
                Pasted::YahooNumeric("12345".into()),
                "{pasted}"
            );
        }
    }

    #[test]
    fn a_yahoo_link_with_no_league_in_it_says_so_rather_than_guessing() {
        let error = extract_ref("https://football.fantasysports.yahoo.com/f1")
            .expect_err("no league id to find");
        assert!(error.contains("Yahoo link"), "{error}");
        // …and it is not silently retried as a Sleeper id.
        assert!(!error.contains("Sleeper ID"), "{error}");
    }

    #[test]
    fn the_yahoo_url_reader_looks_only_at_the_path() {
        assert_eq!(yahoo_url_id("/f1/12345?x=999999"), Some("12345".into()));
        assert_eq!(yahoo_url_id("/f1/#98765"), None);
        // A number too long to be a league id is not one.
        assert_eq!(yahoo_url_id("/f1/1389710366300200960"), None);
    }

    #[test]
    fn a_sleeper_paste_still_reads_as_one() {
        assert_eq!(
            extract_ref("https://sleeper.com/draft/nfl/1389710366300200960").unwrap(),
            Pasted::Sleeper("1389710366300200960".into())
        );
    }
}
