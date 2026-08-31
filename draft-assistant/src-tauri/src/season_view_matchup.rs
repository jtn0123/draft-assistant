//! This week's head-to-head section of the season view: the two lineups side
//! by side, and the start/sit calls that close the gap between my best lineup
//! and the one I have set.
//!
//! Everything downstream of the matchup — the roster table, the waiver and
//! trade searches, the header — needs pieces this section works out along the
//! way, so [`MatchupSection`] hands them back rather than recomputing them.

use crate::engine::{now_secs, LoadedLeague};
use crate::roster::RosterRules;
use crate::season_api::{matchup_for, opponent_of, Matchup, Roster};
use crate::season_calls;
use crate::season_engine::LoadedSeason;
use crate::season_injury::injury_code;
use crate::season_lineup::{
    calls_from_diff, candidates_for, optimal_lineup, Candidate, LineupCall, LineupSlot,
};
use crate::season_lookup::Lookup;
use crate::season_types::{MatchupRow, MatchupView};
use crate::weekly::WeeklyPoints;

/// The head-to-head section, plus the working values later sections reuse.
pub struct MatchupSection<'a> {
    pub my_matchup: Option<&'a Matchup>,
    pub opp_matchup: Option<&'a Matchup>,
    pub matchup: Option<MatchupView>,
    pub calls: Vec<LineupCall>,
    pub points_on_table: f64,
    /// My best possible lineup's projection, and the one my opponent has set.
    pub my_projected: f64,
    pub opp_projected: f64,
    /// The lineups actually set, which the live scoreboard and the roster
    /// table read to label each player's slot.
    pub my_current: Vec<LineupSlot>,
    pub opp_current: Vec<LineupSlot>,
    /// Every player on my roster, scored for this week — the starting point
    /// for the waiver and trade searches too.
    pub my_candidates: Vec<Candidate>,
}

/// Build the head-to-head section for `my_roster`.
pub fn build_matchup<'a>(
    loaded: &LoadedLeague,
    season: &'a LoadedSeason,
    lookup: &Lookup,
    my_roster: Option<&Roster>,
    team_name: &impl Fn(u32) -> String,
    team_avatar: &impl Fn(u32) -> Option<String>,
) -> MatchupSection<'a> {
    let rules = &loaded.roster_rules;
    let weekly = &loaded.weekly_points;
    let week = season.week;
    let position_of = |id: &str| lookup.position(id);
    let projected = |id: &str| weekly.get_or_zero(id, week);

    let my_matchup = my_roster.and_then(|r| matchup_for(&season.matchups, r.roster_id));
    let opp_matchup = my_matchup.and_then(|mine| opponent_of(&season.matchups, mine));

    let my_candidates: Vec<Candidate> = my_roster
        .map(|r| candidates_for(r.player_ids(), &position_of, weekly, week))
        .unwrap_or_default();
    let my_optimal = optimal_lineup(rules, &my_candidates);
    let my_current = my_matchup
        .map(|m| current_lineup(loaded, m.starter_ids(), &projected))
        .unwrap_or_else(|| my_optimal.clone());

    let describe = |id: &str| (lookup.name(id), lookup.team(id));
    let reason = |_slot: &str, player_in: &str, player_out: &str| {
        why_start(lookup, weekly, week, player_in, player_out)
    };
    let eligible = |slot: &str, id: &str| {
        position_of(id).is_some_and(|position| RosterRules::can_fill(slot, &position))
    };
    let mut calls = calls_from_diff(&my_optimal, &my_current, &eligible, &describe, &reason);
    // Rust's additive identity for f64 is -0.0, so an empty sum serialises as
    // "-0.0" and would render as "−0.0 points on the table". Normalise it.
    //
    // Counted before the injury calls join the list: those are about a player
    // who may not take the field at all, not about points being left on the
    // bench, and their gain is often negative.
    let points_on_table: f64 = calls.iter().map(|c| c.gain).sum::<f64>() + 0.0;

    let facts = season_calls::WeekFacts {
        players: lookup,
        weekly,
        week,
        scores: &season.scores,
        now_ms: i64::try_from(now_secs())
            .unwrap_or(i64::MAX)
            .saturating_mul(1000),
    };
    let sidelined = facts.injury_calls(&my_current, &my_candidates, &calls, &eligible);
    calls.extend(sidelined);
    facts.finish(&mut calls);

    let opp_candidates: Vec<Candidate> = opp_matchup
        .and_then(|m| {
            season
                .rosters
                .iter()
                .find(|r| r.roster_id == m.roster_id)
                .map(|r| candidates_for(r.player_ids(), &position_of, weekly, week))
        })
        .unwrap_or_default();
    let opp_optimal = optimal_lineup(rules, &opp_candidates);
    let opp_current = opp_matchup
        .map(|m| current_lineup(loaded, m.starter_ids(), &projected))
        .unwrap_or_else(|| opp_optimal.clone());

    // The comparison shows my best lineup against their set one: I can change
    // mine, I cannot change theirs.
    let my_projected: f64 = my_optimal.iter().map(|s| s.points).sum::<f64>() + 0.0;
    let opp_projected: f64 = opp_current.iter().map(|s| s.points).sum::<f64>() + 0.0;

    // Both halves of the comparison: my best lineup and the one I have set,
    // each against their set one. The screen toggles between them.
    let rows_against_theirs = |mine: &[LineupSlot]| matchup_rows(lookup, mine, &opp_current);

    let matchup = my_matchup.map(|mine| MatchupView {
        my_name: team_name(mine.roster_id),
        opp_name: opp_matchup
            .map(|m| team_name(m.roster_id))
            .unwrap_or_else(|| "Bye week".to_string()),
        my_avatar: team_avatar(mine.roster_id),
        opp_avatar: opp_matchup.and_then(|m| team_avatar(m.roster_id)),
        my_projected,
        opp_projected,
        rows: rows_against_theirs(&my_optimal),
        set_rows: rows_against_theirs(&my_current),
        set_projected: my_current.iter().map(|s| s.points).sum(),
    });

    MatchupSection {
        my_matchup,
        opp_matchup,
        matchup,
        calls,
        points_on_table,
        my_projected,
        opp_projected,
        my_current,
        opp_current,
        my_candidates,
    }
}

/// My lineup, slot by slot, against the one the opponent has set — the rows
/// behind both halves of the head-to-head table.
pub fn matchup_rows(
    lookup: &Lookup,
    mine: &[LineupSlot],
    theirs: &[LineupSlot],
) -> Vec<MatchupRow> {
    let describe = |id: Option<&str>| {
        (
            id.map(|id| lookup.name(id)).unwrap_or_default(),
            id.and_then(|id| lookup.team(id)),
            id.and_then(|id| injury_code(lookup.injury(id).as_deref()))
                .map(str::to_string),
        )
    };
    mine.iter()
        .enumerate()
        .map(|(i, slot)| {
            let opp = theirs.get(i);
            let opp_id = opp.and_then(|s| s.player_id.clone());
            let opp_points = opp.map_or(0.0, |s| s.points);
            let (my_name, my_team, my_injury) = describe(slot.player_id.as_deref());
            let (opp_name, opp_team, opp_injury) = describe(opp_id.as_deref());
            MatchupRow {
                slot: slot.slot.clone(),
                my_name,
                my_team,
                my_injury,
                my_points: slot.points,
                my_player_id: slot.player_id.clone(),
                opp_name,
                opp_team,
                opp_injury,
                opp_points,
                opp_player_id: opp_id,
                margin: slot.points - opp_points,
            }
        })
        .collect()
}

/// The lineup a roster currently has set, slot by slot, in league slot order.
pub fn current_lineup(
    loaded: &LoadedLeague,
    starters: &[String],
    points_of: &impl Fn(&str) -> f64,
) -> Vec<LineupSlot> {
    // Sleeper returns starters positionally against roster_positions, with
    // "0" marking an empty slot.
    let starting_slots: Vec<&String> = loaded
        .roster_rules
        .slots()
        .iter()
        .filter(|s| !RosterRules::is_non_starting(s))
        .collect();
    starting_slots
        .iter()
        .enumerate()
        .map(|(i, slot)| {
            let player_id = starters
                .get(i)
                .filter(|id| !id.is_empty() && id.as_str() != "0")
                .cloned();
            let points = player_id.as_deref().map(points_of).unwrap_or(0.0);
            LineupSlot {
                slot: (*slot).clone(),
                player_id,
                points,
            }
        })
        .collect()
}

/// The prose behind one start/sit call: the projection difference, in words.
pub fn why_start(
    lookup: &Lookup,
    weekly: &WeeklyPoints,
    week: u32,
    player_in: &str,
    player_out: &str,
) -> String {
    let in_points = weekly.get(player_in, week).unwrap_or(0.0);
    let out_bye = !player_out.is_empty() && weekly.is_bye(player_out, week);
    if out_bye {
        return format!(
            "{} is on bye this week \u{2014} anyone projected above zero beats an empty slot.",
            lookup.name(player_out)
        );
    }
    if player_out.is_empty() {
        return format!(
            "{} projects {in_points:.1} into a slot you have left empty.",
            lookup.name(player_in)
        );
    }
    let out_points = weekly.get(player_out, week).unwrap_or(0.0);
    format!(
        "{} projects {in_points:.1} against {:.1} for {} \u{2014} a {:+.1} swing on this week's projection.",
        lookup.name(player_in),
        out_points,
        lookup.name(player_out),
        in_points - out_points
    )
}
