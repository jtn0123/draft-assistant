//! Pulling a Sleeper id out of whatever the user pasted: a bare id, a
//! league URL, a draft URL. Lifted out of `sleeper.rs` for the 500-line cap.

/// Pull the Sleeper ID out of whatever the user pasted — a bare ID or a full
/// URL like https://sleeper.com/draft/nfl/139888...?ftue=commish. Sleeper IDs
/// are 18–19 digit snowflakes; anything shorter is handed back untouched so
/// the API can reject it with its own message.
pub fn extract_id(input: &str) -> String {
    input
        .split(|c: char| !c.is_ascii_digit())
        .max_by_key(|run| run.len())
        .filter(|run| run.len() >= 15)
        .unwrap_or(input.trim())
        .to_string()
}

#[cfg(test)]
mod extract_id_tests {
    use super::extract_id;

    #[test]
    fn a_bare_id_passes_through() {
        assert_eq!(extract_id("1389710366300200961"), "1389710366300200961");
        assert_eq!(extract_id("  1389710366300200961\n"), "1389710366300200961");
    }

    #[test]
    fn a_draft_url_with_query_yields_the_draft_id() {
        assert_eq!(
            extract_id("https://sleeper.com/draft/nfl/1389710366300200961?ftue=commish"),
            "1389710366300200961"
        );
    }

    #[test]
    fn a_league_url_yields_the_league_id() {
        assert_eq!(
            extract_id("https://sleeper.com/leagues/1389710366300200960/team"),
            "1389710366300200960"
        );
    }

    #[test]
    fn the_longest_digit_run_wins_over_short_ones() {
        // `nfl` sits between a year-ish number and the real ID.
        assert_eq!(
            extract_id("2026/nfl/1389710366300200961/1"),
            "1389710366300200961"
        );
    }

    #[test]
    fn a_short_string_is_returned_trimmed_for_the_api_to_reject() {
        assert_eq!(extract_id(" abc123 "), "abc123");
    }
}
