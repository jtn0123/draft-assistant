//! The one reading of an `injury_status` string, Sleeper's or Yahoo's.
//!
//! Two dictionaries used to exist: the draft recommender knew "SUS" and
//! "COV", the season screen knew "sus", "susp", "suspended", "cov" and
//! "covid". A player tagged "Suspended" was therefore sidelined on the season
//! screen and priced as an unfamiliar weekly tag, six points, on the draft
//! card, when a suspension takes weeks of the season away. One classifier,
//! read by both, so a spelling learnt in one place is known in the other.
//!
//! Yahoo's codes are in the same table. A Yahoo player carries `status` as a
//! short code ("Q", "O", "D", "IR-R", "PUP-R", "NFI-R") and the board keeps
//! it as it came, so until these were here every Yahoo "Q" fell through to
//! the unfamiliar-tag price and was docked six points all night.

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

/// `None` for an empty or blank status, which is how both platforms say
/// "healthy".
pub fn classify(status: &str) -> Option<InjuryClass> {
    let code = status.trim().to_ascii_lowercase();
    Some(match code.as_str() {
        "" => return None,
        "ir" | "pup" | "pup-p" | "na" | "dnr" => InjuryClass::SeasonEnding,
        "out" | "o" => InjuryClass::OneWeek,
        // The reserve lists a player is designated to return from: Yahoo's
        // "-R" suffix means at least four games away, not the season.
        "sus" | "susp" | "sspd" | "suspended" | "cov" | "covid" | "ir-r" | "pup-r" | "nfi-r" => {
            InjuryClass::MultiWeek
        }
        "doubtful" | "d" => InjuryClass::Doubtful,
        "questionable" | "q" => InjuryClass::Questionable,
        _ => InjuryClass::Unknown,
    })
}

/// A physically-unable-to-perform or non-football-injury list, which before
/// the season starts is camp news and not a season lost.
///
/// A player on PUP in August has not been ruled out of anything yet: most
/// come off it at the roster cut, and one who stays on it misses the first
/// four games. The table above prices PUP as season-ending, which is right
/// once the season is under way (an in-season PUP move is at least four games
/// and usually the year) and wrong in a draft room, where it priced a
/// first-round back as a lost season for a hamstring in training camp. The
/// recommender asks this before the season starts and prices the early weeks
/// instead.
pub fn is_camp_reserve(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "pup" | "pup-p" | "pup-r" | "nfi" | "nfi-r"
    )
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

    #[test]
    fn yahoos_short_codes_mean_the_same_as_sleepers_words() {
        // The failure this prevents: a Yahoo player's `status` is "Q", not
        // "Questionable", and the board keeps it as it came. The recommender
        // read every one of them as an unfamiliar tag and took six points
        // off, three times what the practice report is worth, while "PUP-R"
        // and "IR-R", both a return in a few weeks, stayed on the card as
        // unknowns too.
        for (code, want) in [
            ("Q", InjuryClass::Questionable),
            ("D", InjuryClass::Doubtful),
            ("O", InjuryClass::OneWeek),
            ("IR", InjuryClass::SeasonEnding),
            ("IR-R", InjuryClass::MultiWeek),
            ("PUP-P", InjuryClass::SeasonEnding),
            ("PUP-R", InjuryClass::MultiWeek),
            ("NFI-R", InjuryClass::MultiWeek),
            ("SUSP", InjuryClass::MultiWeek),
            ("SSPD", InjuryClass::MultiWeek),
            ("NA", InjuryClass::SeasonEnding),
        ] {
            assert_eq!(classify(code), Some(want), "for {code:?}");
            // Yahoo sends them upper-case; nothing may depend on that.
            assert_eq!(
                classify(&code.to_ascii_lowercase()),
                Some(want),
                "for {code:?} in lower case"
            );
        }
    }

    #[test]
    fn only_the_camp_lists_are_camp_news() {
        for status in ["PUP", "pup", "PUP-P", "PUP-R", "NFI-R", " pup "] {
            assert!(is_camp_reserve(status), "{status:?} is a camp list");
        }
        for status in ["IR", "IR-R", "Out", "O", "Q", "Suspended", "NA", ""] {
            assert!(!is_camp_reserve(status), "{status:?} is not a camp list");
        }
    }
}
