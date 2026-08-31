//! The live-scoring section of the season view: this week's real NFL games,
//! filtered to the ones a player in either set lineup is actually in.
//!
//! It takes the two lineups the matchup section already worked out rather than
//! deriving them again, so a slot label on the scoreboard always agrees with
//! the slot label in the head-to-head table.

use crate::season_api::Matchup;
use crate::season_engine::LoadedSeason;
use crate::season_lineup::LineupSlot;
use crate::season_live::{self, TrackedPlayer};
use crate::season_lookup::Lookup;
use crate::season_types::LiveSection;
use crate::weekly::WeeklyPoints;
use std::collections::HashMap;

/// One side of the matchup: whose entry it is and the lineup they have set.
pub struct LiveSide<'a> {
    pub matchup: Option<&'a Matchup>,
    pub lineup: &'a [LineupSlot],
}

/// Build the scoreboard for the players in this week's two lineups.
pub fn live_section(
    season: &LoadedSeason,
    lookup: &Lookup,
    weekly: &WeeklyPoints,
    mine: LiveSide,
    theirs: LiveSide,
) -> LiveSection {
    let week = season.week;
    let projected = |id: &str| weekly.get_or_zero(id, week);
    let mut tracked: Vec<TrackedPlayer> = Vec::new();
    for (side, is_mine) in [(mine, true), (theirs, false)] {
        let Some(matchup) = side.matchup else {
            continue;
        };
        let slot_of: HashMap<&str, &str> = side
            .lineup
            .iter()
            .filter_map(|s| Some((s.player_id.as_deref()?, s.slot.as_str())))
            .collect();
        for player_id in matchup.starter_ids() {
            if player_id.is_empty() || player_id == "0" {
                continue;
            }
            tracked.push(TrackedPlayer {
                slot: slot_of
                    .get(player_id.as_str())
                    .map(|s| (*s).to_string())
                    .or_else(|| lookup.position(player_id))
                    .unwrap_or_default(),
                name: lookup.name(player_id),
                team: lookup.team(player_id),
                points: matchup
                    .points_for(player_id)
                    .unwrap_or_else(|| projected(player_id)),
                player_id: player_id.clone(),
                is_mine,
            });
        }
    }
    let games = season_live::live_games(&season.scores, &tracked);
    let windows = season_live::windows(&games);
    let totals = season_live::totals(&games);
    LiveSection {
        next_kickoff_ms: season_live::next_window(&windows).map(|w| w.kickoff_ms),
        games,
        windows,
        totals,
        bye_teams: season_live::bye_teams(&season.scores),
    }
}
