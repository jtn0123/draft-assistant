//! League settings synthesized for Sleeper mock drafts that have no league.

use crate::sleeper::{Draft, League};
use std::collections::HashMap;

/// Receptions under a Sleeper `scoring_type`, or `None` when the value is not
/// one this understands.
///
/// Sleeper spells the mode with the format in front of it — `dynasty_half_ppr`,
/// `rookie_ppr`, `idp_ppr` — so an exact match recognised only the three plain
/// values and quietly scored every other league as standard, which is a full
/// point per catch out on a PPR board. `half_ppr` is tested first because it
/// contains `ppr`.
pub(crate) fn points_per_reception(scoring_type: &str) -> Option<f64> {
    let lowered = scoring_type.to_ascii_lowercase();
    if lowered.contains("half_ppr") {
        Some(0.5)
    } else if lowered.contains("ppr") {
        Some(1.0)
    } else if lowered.contains("std") {
        Some(0.0)
    } else {
        None
    }
}

/// The league a mock draft would have had, plus anything the user needs told
/// about the guesses that went into it.
///
/// The warning is returned rather than printed: a mock scored as standard when
/// it is not moves every pass catcher on the board, and nobody reads stderr.
pub fn synthesize_league(draft: &Draft) -> (League, Option<String>) {
    let settings = &draft.settings;
    let mut roster_positions = Vec::new();
    let mut push_slots = |position: &str, count: Option<u32>| {
        for _ in 0..count.unwrap_or(0) {
            roster_positions.push(position.to_string());
        }
    };
    push_slots("QB", settings.slots_qb);
    push_slots("RB", settings.slots_rb);
    push_slots("WR", settings.slots_wr);
    push_slots("TE", settings.slots_te);
    push_slots("FLEX", settings.slots_flex);
    push_slots("SUPER_FLEX", settings.slots_super_flex);
    push_slots("K", settings.slots_k);
    push_slots("DEF", settings.slots_def);
    let starters = roster_positions.len() as u32;
    for _ in 0..settings.rounds.saturating_sub(starters) {
        roster_positions.push("BN".to_string());
    }

    let scoring_type = draft
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.scoring_type.clone())
        .unwrap_or_else(|| "ppr".into());
    let (points_per_reception, warning) = match points_per_reception(&scoring_type) {
        Some(per_reception) => (per_reception, None),
        None => (
            0.0,
            Some(format!(
                "scoring type '{scoring_type}' not recognised — scored as standard; \
                 check the board's points"
            )),
        ),
    };
    let scoring_settings: HashMap<String, f64> = [
        ("pass_yd", 0.04),
        ("pass_td", 4.0),
        ("pass_int", -1.0),
        ("pass_2pt", 2.0),
        ("rush_yd", 0.1),
        ("rush_td", 6.0),
        ("rush_2pt", 2.0),
        ("rec_yd", 0.1),
        ("rec_td", 6.0),
        ("rec_2pt", 2.0),
        ("rec", points_per_reception),
        ("fum_lost", -2.0),
        ("sack", 1.0),
        ("int", 2.0),
        ("fum_rec", 2.0),
        ("def_td", 6.0),
        ("safe", 2.0),
        ("blk_kick", 2.0),
        ("def_st_td", 6.0),
        ("pts_allow_0", 10.0),
        ("pts_allow_1_6", 7.0),
        ("pts_allow_7_13", 4.0),
        ("pts_allow_14_20", 1.0),
        ("pts_allow_21_27", 0.0),
        ("pts_allow_28_34", -1.0),
        ("pts_allow_35p", -4.0),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value))
    .collect();

    let name = draft
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.name.clone())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| format!("Mock draft ({scoring_type})"));
    let league = League {
        league_id: draft.draft_id.clone(),
        name,
        season: draft.season.clone().unwrap_or_else(|| "2026".into()),
        status: draft.status.clone(),
        total_rosters: settings.teams,
        roster_positions,
        scoring_settings,
        draft_id: Some(draft.draft_id.clone()),
        // A mock draft has no league behind it, so there is no prior season
        // and none of the in-season settings apply.
        previous_league_id: None,
        settings: crate::sleeper::LeagueSettings::default(),
    };
    (league, warning)
}

#[cfg(test)]
mod tests {
    use super::{points_per_reception, synthesize_league};
    use crate::sleeper::Draft;

    fn mock_draft(scoring_type: &str) -> Draft {
        serde_json::from_str(&format!(
            r#"{{"draft_id": "mock-1", "status": "pre_draft", "type": "snake",
                 "settings": {{"teams": 12, "rounds": 5, "slots_qb": 1}},
                 "metadata": {{"scoring_type": "{scoring_type}"}},
                 "season": "2026"}}"#
        ))
        .expect("draft fixture")
    }

    #[test]
    fn an_unrecognised_scoring_type_comes_back_as_a_warning_not_a_printout() {
        let (league, warning) = synthesize_league(&mock_draft("tiered_reception"));
        assert_eq!(league.scoring_settings.get("rec"), Some(&0.0));
        let warning = warning.expect("the assumption has to be reported");
        assert!(warning.contains("tiered_reception"), "{warning}");
        assert!(warning.contains("scored as standard"), "{warning}");
    }

    #[test]
    fn a_recognised_scoring_type_has_nothing_to_warn_about() {
        let (league, warning) = synthesize_league(&mock_draft("dynasty_half_ppr"));
        assert_eq!(league.scoring_settings.get("rec"), Some(&0.5));
        assert_eq!(warning, None);
    }

    #[test]
    fn the_format_in_front_of_the_mode_does_not_hide_the_mode() {
        assert_eq!(points_per_reception("ppr"), Some(1.0));
        assert_eq!(points_per_reception("half_ppr"), Some(0.5));
        assert_eq!(points_per_reception("std"), Some(0.0));
        // The prefixed spellings Sleeper actually serves. half_ppr wins over
        // ppr, which it contains — a half-PPR mock scored as full PPR moves
        // every pass catcher up the board.
        assert_eq!(points_per_reception("dynasty_half_ppr"), Some(0.5));
        assert_eq!(points_per_reception("dynasty_ppr"), Some(1.0));
        assert_eq!(points_per_reception("rookie_ppr"), Some(1.0));
        assert_eq!(points_per_reception("2qb_std"), Some(0.0));
    }

    #[test]
    fn a_scoring_type_this_does_not_know_is_said_so_rather_than_assumed() {
        assert_eq!(points_per_reception("tiered_reception"), None);
        assert_eq!(points_per_reception(""), None);
    }
}
