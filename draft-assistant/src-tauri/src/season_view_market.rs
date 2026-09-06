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
use crate::season_trades::{self, PlayerDesc, TradeIdea, TradePartner};

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
    let last_regular = loaded.league.last_regular_week().max(week);
    // The board arrives in season-rank order, but the gain below is measured
    // against a weekly projection. Rank the free agents on that first, or the
    // hot streamer with a poor season rank falls outside the pool and is
    // never looked at. Cutting the pool here also means only the players we
    // actually evaluate ever become `FreeAgent`s.
    //
    // The rank is the better of this week and the rest of the season, and a
    // player on bye is valued at his rest-of-season rate. Ranked on this week
    // alone, every free agent on bye projected zero, was cut from the pool
    // before evaluation, and the best add on the wire vanished from the
    // waiver list for exactly the week he was cheapest to claim.
    let value_of = |p: &BoardPlayer| -> f64 {
        let rest = weekly.mean_from(&p.player_id, week, last_regular);
        if weekly.is_bye(&p.player_id, week) {
            rest
        } else {
            weekly.get_or_zero(&p.player_id, week).max(rest)
        }
    };
    let mut ranked: Vec<(&BoardPlayer, f64)> = loaded
        .board
        .iter()
        .filter(|p| !rostered.contains(&p.player_id))
        .map(|p| (p, value_of(p)))
        .collect();
    if ranked.len() > CANDIDATE_POOL {
        ranked.select_nth_unstable_by(CANDIDATE_POOL, |a, b| b.1.total_cmp(&a.1));
        ranked.truncate(CANDIDATE_POOL);
    }
    let free_agents: Vec<FreeAgent> = ranked
        .into_iter()
        .map(|(p, _)| FreeAgent {
            player_id: p.player_id.clone(),
            name: p.name.clone(),
            position: p.position.clone(),
            team: p.team.clone(),
            // Scored on this week, as the gain is; a bye player is scored on
            // the weeks he will actually play so the list can still name him.
            weekly_points: if weekly.is_bye(&p.player_id, week) {
                weekly.mean_from(&p.player_id, week, last_regular)
            } else {
                weekly.get_or_zero(&p.player_id, week)
            },
        })
        .collect();
    // Rivals are measured on the players they can start, IR slot excluded.
    let rival_active: Vec<(u32, Vec<String>)> = season
        .rosters
        .iter()
        .filter(|r| Some(r.roster_id) != my_roster_id)
        .map(|r| (r.roster_id, r.active_player_ids()))
        .collect();
    let rival_rosters: Vec<RivalRoster> = rival_active
        .iter()
        .map(|(roster_id, player_ids)| RivalRoster {
            roster_id: *roster_id,
            player_ids,
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
                // Nobody on injured reserve is a trade chip either way.
                candidates_of(&r.active_player_ids()),
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
    season_trades::trade_ideas(rules, my_candidates, &partners, &|id| PlayerDesc {
        name: lookup.name(id),
        position: lookup.position(id).unwrap_or_default(),
        team: lookup.team(id),
    })
}
