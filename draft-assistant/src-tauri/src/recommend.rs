//! Deterministic, auditable pick recommendations, in three modes.
//!
//! `safe`, `balanced` and `upside` are three readings of the same board, and
//! the panel shows all three side by side — so they have to genuinely
//! disagree. Safe stays near the market and marks injuries down hard; balanced
//! is the honest VORP-and-need answer; upside pays for week-to-week variance
//! and for the players this board likes more than the market does.
//!
//! The scoring itself lives next door in `recommend_score.rs`; this file is
//! the inputs, the candidate pool and the loop over the modes.

use crate::board::AvailablePlayer;
use crate::draft::TeamRoster;
use crate::roster::RosterRules;
use crate::view_types::PositionRun;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[path = "recommend_score.rs"]
mod score;
use score::{score_candidate, Context, Score};

#[derive(Debug, Clone, Serialize)]
pub struct Recommendation {
    pub mode: String,
    pub player_id: String,
    pub name: String,
    pub position: String,
    pub team: Option<String>,
    pub points: f64,
    pub vorp: f64,
    pub tier: u32,
    pub adp: Option<f64>,
    pub survival_next: Option<f64>,
    pub score: f64,
    pub reasons: Vec<String>,
}

/// The reception value the imported second opinion's own ranks are built on.
/// The default for a caller that has no league scoring to hand, because at
/// this value the second opinion needs no discount at all.
pub const HALF_PPR: f64 = 0.5;

/// The three readings of the board, in the order the panel lays them out.
pub const MODES: [&str; 3] = ["balanced", "safe", "upside"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Balanced,
    Safe,
    Upside,
}

impl Mode {
    fn label(self) -> &'static str {
        match self {
            Mode::Balanced => "balanced",
            Mode::Safe => "safe",
            Mode::Upside => "upside",
        }
    }
}

/// Everything the scorer reads. A struct rather than a dozen positional
/// arguments because the strategy layer keeps adding to it — the run on a
/// position, the byes already stacked on my starters, whether the draft has
/// even started — and each addition used to mean editing every call site.
pub struct RecommendInputs<'a> {
    pub available: &'a [AvailablePlayer],
    pub my_roster: Option<&'a TeamRoster>,
    pub rules: &'a RosterRules,
    pub current_round: u32,
    pub total_rounds: u32,
    pub current_pick: u32,
    /// Where `current_pick` sits in the market an ADP is measured in — the
    /// count of selections, not of pick numbers. Every comparison against an
    /// ADP has to be made here rather than at `current_pick`: a keeper league
    /// enters twenty-six picks before anybody is on the clock, and reading
    /// pick 30 as market position 30 told the recommender that an ADP-12
    /// player had fallen eighteen picks when four names had been called.
    /// Equal to `current_pick` in a league with no keepers.
    pub market_pick: u32,
    /// League size, so the imported second opinion can say how many rounds
    /// late the market is rather than how many picks, and so the pick
    /// thresholds that used to assume twelve teams scale with the real one.
    pub teams: u32,
    /// What this league pays for a catch. The imported second opinion's ranks
    /// are half-PPR; how far to trust them depends on how far the league is
    /// from that.
    pub points_per_reception: f64,
    /// The run under way, if any: four of a position in the last six picks.
    pub position_run: Option<&'a PositionRun>,
    /// Bye week -> how many of my starters already have it. Built by the
    /// caller, which is the only place that can look a rostered player's bye
    /// up on the board.
    pub my_byes: &'a HashMap<u32, u32>,
    /// The draft has not started. Weekly practice tags ("Questionable") are
    /// left over from last season and mean nothing yet.
    pub pre_draft: bool,
}

impl<'a> RecommendInputs<'a> {
    /// The plain form: no strategy layer, for callers (mostly tests) that
    /// only have a board and a roster.
    pub fn new(
        available: &'a [AvailablePlayer],
        my_roster: Option<&'a TeamRoster>,
        rules: &'a RosterRules,
        current_round: u32,
        total_rounds: u32,
        current_pick: u32,
        teams: u32,
    ) -> Self {
        static NO_BYES: std::sync::LazyLock<HashMap<u32, u32>> =
            std::sync::LazyLock::new(HashMap::new);
        RecommendInputs {
            available,
            my_roster,
            rules,
            current_round,
            total_rounds,
            current_pick,
            market_pick: current_pick,
            teams,
            points_per_reception: HALF_PPR,
            position_run: None,
            my_byes: &NO_BYES,
            pre_draft: false,
        }
    }
}

pub fn recommend(inputs: &RecommendInputs) -> Vec<Recommendation> {
    let open: HashMap<String, u32> = inputs
        .my_roster
        .map(|r| r.open_starters.iter().cloned().collect())
        .unwrap_or_else(|| {
            inputs
                .rules
                .slots()
                .iter()
                .filter(|slot| !RosterRules::is_non_starting(slot))
                .fold(HashMap::new(), |mut m, s| {
                    *m.entry(s.clone()).or_insert(0) += 1;
                    m
                })
        });
    let rounds_left = inputs
        .total_rounds
        .saturating_sub(inputs.current_round)
        .saturating_add(1);
    let total_open: u32 = open.values().sum();
    // When open starting slots ~= rounds left, filling starters is urgent.
    let need_pressure = total_open as f64 / rounds_left.max(1) as f64;

    // How many of each position I already roster — backup discipline depends
    // on it (one DEF ever, one QB/TE unless value, diminishing flex depth).
    let mut have: HashMap<&str, u32> = HashMap::new();
    // NFL teams whose running back I already hold. A second back off the same
    // team is that man's handcuff: cheap insurance on a pick I already made.
    let mut rb_teams: HashSet<&str> = HashSet::new();
    if let Some(roster) = inputs.my_roster {
        for p in &roster.players {
            *have.entry(p.position.as_str()).or_insert(0) += 1;
            if p.position == "RB" {
                if let Some(team) = p.team.as_deref() {
                    rb_teams.insert(team);
                }
            }
        }
    }

    let mut recs: Vec<Recommendation> = Vec::new();
    let candidates = candidates(inputs.available);
    if candidates.is_empty() {
        return recs;
    }

    // The middle of the board's week-to-week spread, so upside can be judged
    // against this source's actual distribution rather than a number picked
    // out of the air.
    let mut spreads: Vec<f64> = inputs
        .available
        .iter()
        .filter_map(|a| a.player.weekly_cv)
        .filter(|cv| *cv > 0.0)
        .collect();
    spreads.sort_by(f64::total_cmp);
    let median_cv = spreads.get(spreads.len() / 2).copied();

    let context = Context {
        inputs,
        open,
        have,
        rb_teams,
        need_pressure,
        rounds_left,
        median_cv,
    };

    for mode in [Mode::Balanced, Mode::Safe, Mode::Upside] {
        let mut best: Option<(Score, &AvailablePlayer)> = None;
        for a in &candidates {
            let Some(scored) = score_candidate(&context, a, mode) else {
                continue;
            };
            if best
                .as_ref()
                .map(|(s, _)| scored.total > s.total)
                .unwrap_or(true)
            {
                best = Some((scored, a));
            }
        }
        // Safety net: if every candidate was disqualified (deep-roster edge
        // cases), fall back to best available so a pick is always suggested.
        let (scored, a) = match best {
            Some(found) => found,
            None => {
                let Some(a) = candidates.first() else {
                    continue;
                };
                (Score::fallback(), *a)
            }
        };
        recs.push(Recommendation {
            mode: mode.label().into(),
            player_id: a.player.player_id.clone(),
            name: a.player.name.clone(),
            position: a.player.position.clone(),
            team: a.player.team.clone(),
            points: a.player.points,
            vorp: a.player.vorp,
            tier: a.player.tier,
            adp: a.player.adp,
            survival_next: a.survival_next,
            score: scored.total,
            reasons: scored.into_reasons(),
        });
    }
    recs
}

/// Top of the overall board PLUS the top few at every position. Overall-only
/// would bury e.g. late-round RBs (negative VORP) under a wall of WRs and
/// never even consider them.
fn candidates(available: &[AvailablePlayer]) -> Vec<&AvailablePlayer> {
    let mut candidates: Vec<&AvailablePlayer> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut per_pos: HashMap<&str, u32> = HashMap::new();
    for (i, a) in available.iter().enumerate() {
        let pos_count = per_pos.entry(a.player.position.as_str()).or_insert(0);
        let take = i < 60 || *pos_count < 10;
        if take && seen.insert(a.player.player_id.as_str()) {
            *pos_count += 1;
            candidates.push(a);
        }
    }
    candidates
}

#[cfg(test)]
#[path = "recommend_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "recommend_mode_tests.rs"]
mod mode_tests;

#[cfg(test)]
#[path = "recommend_league_tests.rs"]
mod league_tests;
