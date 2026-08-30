//! League settings synthesized for Sleeper mock drafts that have no league.

use crate::sleeper::{Draft, League};
use std::collections::HashMap;

pub fn synthesize_league(draft: &Draft) -> League {
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
    let points_per_reception = match scoring_type.as_str() {
        "ppr" => 1.0,
        "half_ppr" => 0.5,
        _ => 0.0,
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
    League {
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
    }
}
