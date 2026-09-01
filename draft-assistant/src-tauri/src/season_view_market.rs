//! The moves-I-could-make section of the season view: waiver targets off the
//! free-agent pool, and trade ideas against every rival roster.
//!
//! Like the standings, neither can change from live scoring — both are
//! searches over rosters and projections — so the poller reuses them.

use crate::board::BoardPlayer;
use crate::engine::LoadedLeague;
use crate::season_api::Roster;
use crate::season_engine::LoadedSeason;
use crate::season_lineup::{candidates_for, Candidate};
use crate::season_lookup::Lookup;
use crate::season_moves::{self, FreeAgent, RivalRoster, WaiverTarget, CANDIDATE_POOL};
use crate::season_trades::{self, TradeIdea, TradePartner};

/// The best available free agents, ranked by what they would add to my lineup.
pub fn waiver_targets(
    loaded: &LoadedLeague,
    season: &LoadedSeason,
    lookup: &Lookup,
    my_roster_id: Option<u32>,
    my_candidates: &[Candidate],
    budget_left: Option<f64>,
) -> Vec<WaiverTarget> {
    let rules = &loaded.roster_rules;
    let weekly = &loaded.weekly_points;
    let week = season.week;
    let position_of = |id: &str| lookup.position(id);
    let sidelined = |id: &str| lookup.is_sidelined(id);
    let candidates_of =
        |ids: &[String]| candidates_for(ids, &position_of, &sidelined, weekly, week);

    let rostered = season_moves::rostered_ids(season.rosters.iter().map(Roster::player_ids));
    // The board arrives in season-rank order, but the gain below is measured
    // against *this week's* projection. Rank the free agents on that first, or
    // the hot streamer with a poor season rank falls outside the pool and is
    // never looked at. Cutting the pool here also means only the players we
    // actually evaluate ever become `FreeAgent`s.
    let mut ranked: Vec<(&BoardPlayer, f64)> = loaded
        .board
        .iter()
        .filter(|p| !rostered.contains(&p.player_id))
        .map(|p| (p, weekly.get_or_zero(&p.player_id, week)))
        .collect();
    if ranked.len() > CANDIDATE_POOL {
        ranked.select_nth_unstable_by(CANDIDATE_POOL, |a, b| b.1.total_cmp(&a.1));
        ranked.truncate(CANDIDATE_POOL);
    }
    let free_agents: Vec<FreeAgent> = ranked
        .into_iter()
        .map(|(p, weekly_points)| FreeAgent {
            player_id: p.player_id.clone(),
            name: p.name.clone(),
            position: p.position.clone(),
            team: p.team.clone(),
            weekly_points,
        })
        .collect();
    let rival_rosters: Vec<RivalRoster> = season
        .rosters
        .iter()
        .filter(|r| Some(r.roster_id) != my_roster_id)
        .map(|r| RivalRoster {
            roster_id: r.roster_id,
            player_ids: r.player_ids(),
        })
        .collect();
    season_moves::waiver_targets(
        rules,
        my_candidates,
        &free_agents,
        &rival_rosters,
        &candidates_of,
        budget_left,
    )
}

/// Trade ideas against every rival roster.
pub fn trade_ideas(
    loaded: &LoadedLeague,
    season: &LoadedSeason,
    lookup: &Lookup,
    my_roster_id: Option<u32>,
    my_candidates: &[Candidate],
    team_name: &impl Fn(u32) -> String,
) -> Vec<TradeIdea> {
    let rules = &loaded.roster_rules;
    let weekly = &loaded.weekly_points;
    let week = season.week;
    let position_of = |id: &str| lookup.position(id);
    let sidelined = |id: &str| lookup.is_sidelined(id);
    let candidates_of =
        |ids: &[String]| candidates_for(ids, &position_of, &sidelined, weekly, week);

    let partner_candidates: Vec<(u32, String, Vec<Candidate>)> = season
        .rosters
        .iter()
        .filter(|r| Some(r.roster_id) != my_roster_id)
        .map(|r| {
            (
                r.roster_id,
                team_name(r.roster_id),
                candidates_of(r.player_ids()),
            )
        })
        .collect();
    let partners: Vec<TradePartner> = partner_candidates
        .iter()
        .map(|(roster_id, name, candidates)| TradePartner {
            roster_id: *roster_id,
            name: name.clone(),
            candidates,
        })
        .collect();
    season_trades::trade_ideas(rules, my_candidates, &partners, &|id| {
        (lookup.name(id), lookup.position(id).unwrap_or_default())
    })
}
