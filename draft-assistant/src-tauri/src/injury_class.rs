//! The one reading of Sleeper's `injury_status` strings.
//!
//! Two dictionaries used to exist: the draft recommender knew "SUS" and
//! "COV", the season screen knew "sus", "susp", "suspended", "cov" and
//! "covid". A player tagged "Suspended" was therefore sidelined on the season
//! screen and priced as an unfamiliar weekly tag, six points, on the draft
//! card, when a suspension takes weeks of the season away. One classifier,
//! read by both, so a spelling learnt in one place is known in the other.

/// What a tag means for the games, which is what both readers care about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjuryClass {
    /// Out for the year, or as near as makes no difference.
    SeasonEnding,
    /// A ruling for one game.
    OneWeek,
    /// A suspension or a reserve list: a length measured in weeks.
    MultiWeek,
    /// Practice-report tags, about Sunday and not about the season.
    Doubtful,
    Questionable,
    /// A tag neither reader has seen. Still a tag; Sleeper adds them.
    Unknown,
}

/// `None` for an empty or blank status, which is how Sleeper says "healthy".
pub fn classify(status: &str) -> Option<InjuryClass> {
    let code = status.trim().to_ascii_lowercase();
    Some(match code.as_str() {
        "" => return None,
        "ir" | "pup" | "na" | "dnr" => InjuryClass::SeasonEnding,
        "out" => InjuryClass::OneWeek,
        "sus" | "susp" | "suspended" | "cov" | "covid" => InjuryClass::MultiWeek,
        "doubtful" => InjuryClass::Doubtful,
        "questionable" => InjuryClass::Questionable,
        _ => InjuryClass::Unknown,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_status_either_reader_knew_lands_in_the_same_class_for_both() {
        // Every string the two old dictionaries knew between them, in the
        // spellings they knew them by. "Suspended" is the one that fell
        // through: known to the season screen, priced as a weekly tag by the
        // recommender.
        for (status, want) in [
            ("IR", InjuryClass::SeasonEnding),
            ("PUP", InjuryClass::SeasonEnding),
            ("NA", InjuryClass::SeasonEnding),
            ("DNR", InjuryClass::SeasonEnding),
            ("Out", InjuryClass::OneWeek),
            ("OUT", InjuryClass::OneWeek),
            ("Sus", InjuryClass::MultiWeek),
            ("SUS", InjuryClass::MultiWeek),
            ("susp", InjuryClass::MultiWeek),
            ("Suspended", InjuryClass::MultiWeek),
            ("COV", InjuryClass::MultiWeek),
            ("covid", InjuryClass::MultiWeek),
            ("Doubtful", InjuryClass::Doubtful),
            ("DOUBTFUL", InjuryClass::Doubtful),
            ("Questionable", InjuryClass::Questionable),
            ("  Out  ", InjuryClass::OneWeek),
            ("Probable", InjuryClass::Unknown),
        ] {
            assert_eq!(classify(status), Some(want), "for {status:?}");
        }
        assert_eq!(classify(""), None);
        assert_eq!(classify("   "), None);
    }
}
