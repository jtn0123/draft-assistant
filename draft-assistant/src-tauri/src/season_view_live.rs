//! The live-scoring section of the season view: this week's real NFL games,
//! filtered to the ones a player in either set lineup is actually in.
//!
//! It takes the two lineups the matchup section already worked out rather than
//! deriving them again, so a slot label on the scoreboard always agrees with
//! the slot label in the head-to-head table.

use crate::roster::RosterRules;
use crate::season_api::Matchup;
use crate::season_engine::LoadedSeason;
use crate::season_lineup::LineupSlot;
use crate::season_live::{self, TrackedPlayer};
use crate::season_lookup::Lookup;
use crate::season_types::LiveSection;
use crate::weekly::WeeklyPoints;
use std::collections::{HashMap, HashSet};

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
    // Teams whose game has kicked off, final games included.
    let started: HashSet<String> = season_live::remaining_by_team(&season.scores)
        .into_keys()
        .collect();
    let has_started = |id: &str| {
        lookup
            .team(id)
            .is_some_and(|team| started.contains(&team.to_ascii_uppercase()))
    };
    let lineup_locked = mine.matchup.is_some_and(|m| {
        lineup_locked(
            season,
            lookup,
            weekly,
            m.roster_id,
            mine.lineup,
            &has_started,
        )
    });
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
                // Live points once he has kicked off, the projection until
                // then. Sleeper leaves a player with nothing to his name out
                // of `players_points` altogether, so a starter absent from it
                // mid-game has scored nothing; he used to be shown his
                // projection, which inflated the live total by exactly the
                // points he had failed to score.
                points: matchup.points_for(player_id).unwrap_or_else(|| {
                    if has_started(player_id) {
                        0.0
                    } else {
                        projected(player_id)
                    }
                }),
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
        lineup_locked,
    }
}

/// True when nothing about my set lineup can change any more this week.
///
/// A slot is still open while its starter has not kicked off, or, when it is
/// empty or holds a player with no game this week, while someone on my bench
/// who could legally fill it has not kicked off either. The screen used to
/// decide this off the scoreboard chips alone, and an empty or bye slot has
/// no chip: with the rest of the lineup on the field, a FLEX left empty read
/// as locked and the bench player who could still have filled it was never
/// offered.
fn lineup_locked(
    season: &LoadedSeason,
    lookup: &Lookup,
    weekly: &WeeklyPoints,
    my_roster_id: u32,
    lineup: &[LineupSlot],
    has_started: &dyn Fn(&str) -> bool,
) -> bool {
    let starting: HashSet<&str> = lineup
        .iter()
        .filter_map(|s| s.player_id.as_deref())
        .collect();
    // Nothing of mine on the board yet is never locked: a bye week, or a
    // scoreboard that has not loaded.
    if !starting.iter().any(|id| has_started(id)) {
        return false;
    }
    let bench: Vec<String> = season
        .rosters
        .iter()
        .find(|r| r.roster_id == my_roster_id)
        .map(|r| {
            r.active_player_ids()
                .into_iter()
                .filter(|id| !starting.contains(id.as_str()))
                .collect()
        })
        .unwrap_or_default();
    let bench_can_fill = |slot: &str| {
        bench.iter().any(|id| {
            !has_started(id)
                && !weekly.is_bye(id, season.week)
                && lookup
                    .position(id)
                    .is_some_and(|position| RosterRules::can_fill(slot, &position))
        })
    };
    lineup.iter().all(|slot| match slot.player_id.as_deref() {
        Some(id) if has_started(id) => true,
        // A starter still to kick off can always be benched; a starter with
        // no game, or an empty slot, is settled only once nobody can step in.
        Some(id) if lookup.team(id).is_some() && !weekly.is_bye(id, season.week) => false,
        _ => !bench_can_fill(&slot.slot),
    })
}
