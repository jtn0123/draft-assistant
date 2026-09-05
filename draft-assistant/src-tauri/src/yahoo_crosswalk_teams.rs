//! Spelling one NFL team the same way on both sides of the crosswalk.
//!
//! A defence is matched by its team abbreviation and nothing else: Yahoo
//! writes a defence's name as the city ("Jacksonville") and Sleeper splits it
//! across two fields ("Jacksonville", "Jaguars"), so the names never line up.
//! That works right up until the two sources spell the *team* differently,
//! and they do: Yahoo says `JAC` where Sleeper says `JAX`, `WSH` where
//! Sleeper says `WAS`, and a league still carrying an old franchise writes
//! `OAK` or `SD`. Each of those used to leave that league's defence with no
//! Sleeper row at all — on the board with no projection, ranked below every
//! kicker, and counted as an unmatched player.
//!
//! Two fallbacks live here. [`canonical_team`] folds every spelling of a
//! franchise onto Sleeper's, and [`team_words`] gives the city and mascot
//! words a defence can be found by when even the abbreviation is missing —
//! Yahoo leaves `editorial_team_abbr` empty on the odd defence row.

/// Every abbreviation that is not Sleeper's, and the Sleeper one it means.
///
/// Sleeper's own spellings are absent on purpose: anything not in this table
/// is already canonical and passes through. Both live franchises with two
/// current spellings (JAX/JAC, WAS/WSH) and the moved ones (OAK, SD, STL) are
/// here, because a Yahoo league that has run since 2015 still names them.
const ALIASES: &[(&str, &str)] = &[
    ("ARZ", "ARI"),
    ("BLT", "BAL"),
    ("CLV", "CLE"),
    ("GNB", "GB"),
    ("HST", "HOU"),
    ("JAC", "JAX"),
    ("KAN", "KC"),
    ("LA", "LAR"),
    ("STL", "LAR"),
    ("SD", "LAC"),
    ("SDG", "LAC"),
    ("LVR", "LV"),
    ("OAK", "LV"),
    ("NWE", "NE"),
    ("NOR", "NO"),
    ("SFO", "SF"),
    ("TAM", "TB"),
    ("OTI", "TEN"),
    ("CLT", "IND"),
    ("RAV", "BAL"),
    ("WSH", "WAS"),
];

/// One team abbreviation as Sleeper spells it, upper-cased and trimmed.
///
/// An abbreviation nobody knows comes back as itself: a team this table has
/// never heard of still matches the other side when both write it the same
/// way, which is the common case for anything new.
pub fn canonical_team(abbreviation: &str) -> String {
    let upper = abbreviation.trim().to_ascii_uppercase();
    ALIASES
        .iter()
        .find(|(alias, _)| *alias == upper)
        .map(|(_, sleeper)| (*sleeper).to_string())
        .unwrap_or(upper)
}

/// The words a defence can be recognised by: every space-separated piece of
/// the name, lower-cased, with the short ones dropped.
///
/// Both sides are indexed by these, so Sleeper's "Jacksonville" + "Jaguars"
/// and Yahoo's "Jacksonville" meet on `jacksonville`, and a Yahoo row written
/// "Jacksonville Jaguars" meets it on both. Pieces shorter than four letters
/// are dropped because "of", "la" and "ny" identify nothing and would put two
/// franchises on one key.
pub fn team_words(name: &str) -> Vec<String> {
    name.split_whitespace()
        .map(|word| {
            word.chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase()
        })
        .filter(|word| word.len() >= 4)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_spellings_of_one_franchise_fold_together() {
        // The failure this prevents: a Yahoo league writing JAC or WSH had its
        // defence land on the board with no Sleeper row and no projection.
        assert_eq!(canonical_team("JAC"), "JAX");
        assert_eq!(canonical_team("Jac"), "JAX");
        assert_eq!(canonical_team("WSH"), "WAS");
        assert_eq!(canonical_team("LVR"), "LV");
        assert_eq!(canonical_team("OAK"), "LV");
        assert_eq!(canonical_team("SD"), "LAC");
        assert_eq!(canonical_team("STL"), "LAR");
        // Sleeper's own spellings survive untouched.
        for team in ["JAX", "WAS", "LV", "LAC", "LAR", "GB", "KC"] {
            assert_eq!(canonical_team(team), team);
        }
        // And a team nobody has heard of is still itself.
        assert_eq!(canonical_team(" xyz "), "XYZ");
    }

    #[test]
    fn a_defence_is_indexed_by_its_city_and_its_mascot_but_not_by_noise() {
        assert_eq!(
            team_words("Jacksonville Jaguars"),
            ["jacksonville", "jaguars"]
        );
        assert_eq!(team_words("Baltimore"), ["baltimore"]);
        // Two-letter and three-letter pieces identify nothing on their own.
        assert!(team_words("LA Rams").contains(&"rams".to_string()));
        assert!(!team_words("LA Rams").contains(&"la".to_string()));
    }
}
