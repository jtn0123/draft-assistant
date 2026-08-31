//! Sleeper's `injury_status` reduced to something a manager reads at a glance.
//!
//! The dictionary spells the same idea a dozen ways — "IR", "PUP", "Sus", "NA"
//! all mean the player is not taking the field — and none of those letters mean
//! anything to someone who just wants to know whether to start him. Everything
//! downstream works in three tags and the three words behind them.

/// The three tags worth showing, in their shortest form.
pub const OUT: &str = "O";
pub const DOUBTFUL: &str = "D";
pub const QUESTIONABLE: &str = "Q";

/// Sleeper's `injury_status` boiled down to one letter, or nothing at all.
pub fn injury_code(status: Option<&str>) -> Option<&'static str> {
    match status?.trim().to_ascii_lowercase().as_str() {
        "questionable" => Some(QUESTIONABLE),
        "doubtful" => Some(DOUBTFUL),
        "out" | "ir" | "pup" | "sus" | "susp" | "suspended" | "na" | "dnr" | "cov" | "covid" => {
            Some(OUT)
        }
        _ => None,
    }
}

/// The word a tag stands for, wherever there is room to spell it out.
pub fn injury_word(code: &str) -> &'static str {
    match code {
        QUESTIONABLE => "Questionable",
        DOUBTFUL => "Doubtful",
        _ => "Out",
    }
}

/// A tag that means the player probably will not take the field. Questionable
/// deliberately does not count: most of them play.
pub fn is_sidelined(code: Option<&str>) -> bool {
    matches!(code, Some(OUT) | Some(DOUBTFUL))
}

/// What the advice code needs to know about a player. Implemented over the
/// season screen's own `Lookup`, and over a plain map in tests.
pub trait PlayerFacts {
    fn name(&self, player_id: &str) -> String;
    fn team(&self, player_id: &str) -> Option<String>;
    /// Sleeper's raw injury status, before it is boiled down to a tag.
    fn injury_status(&self, player_id: &str) -> Option<String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleeper_injury_spellings_reduce_to_three_tags() {
        for (status, want) in [
            ("Questionable", Some(QUESTIONABLE)),
            ("doubtful", Some(DOUBTFUL)),
            ("Out", Some(OUT)),
            ("IR", Some(OUT)),
            ("PUP", Some(OUT)),
            ("Sus", Some(OUT)),
            ("  Out  ", Some(OUT)),
            ("Probable", None),
            ("", None),
        ] {
            assert_eq!(injury_code(Some(status)), want, "for {status:?}");
        }
        assert_eq!(injury_code(None), None);
    }

    #[test]
    fn every_tag_spells_itself_out() {
        assert_eq!(injury_word(QUESTIONABLE), "Questionable");
        assert_eq!(injury_word(DOUBTFUL), "Doubtful");
        assert_eq!(injury_word(OUT), "Out");
    }

    #[test]
    fn only_out_and_doubtful_count_as_sidelined() {
        assert!(is_sidelined(Some(OUT)));
        assert!(is_sidelined(Some(DOUBTFUL)));
        assert!(!is_sidelined(Some(QUESTIONABLE)), "most of them play");
        assert!(!is_sidelined(None));
    }
}
