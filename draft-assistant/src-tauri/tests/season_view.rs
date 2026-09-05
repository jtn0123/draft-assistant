//! Core assertions on `build_season_view`: matchup rows, start/sit calls,
//! roster roles, standings, waivers, and the header numbers they feed.

mod common;

use draft_assistant_lib::season::{build_season_view, SeasonView, SEASON_SCHEMA_VERSION};

fn view() -> SeasonView {
    let (loaded, season, config) = common::fixture();
    build_season_view(&loaded, &season, config.my_user_id.as_deref())
}

#[test]
fn identifies_my_roster_and_the_league() {
    let v = view();
    assert_eq!(v.schema_version, SEASON_SCHEMA_VERSION);
    assert_eq!(v.my_roster_id, Some(1));
    assert_eq!(v.week, 2);
    assert_eq!(v.season, "2025");
    assert_eq!(v.league.league_id, "league-1");
    assert_eq!(v.league.total_rosters, 4);
    // FLEX takes RB/WR/TE, so a tight end is draftable even with no TE slot.
    assert_eq!(v.league.draftable_positions, vec!["QB", "RB", "WR", "TE"]);
    assert_eq!(v.data_health.warnings, vec!["fixture warning".to_string()]);
}

#[test]
fn matchup_compares_my_best_lineup_against_their_set_one() {
    let v = view();
    let m = v.matchup.expect("my roster has a matchup this week");
    assert_eq!(m.my_name, "User One");
    assert_eq!(m.opp_name, "User Two");
    assert_eq!(m.my_avatar.as_deref(), Some("avatar-one"));
    assert_eq!(m.opp_avatar.as_deref(), Some("avatar-two"));
    // Optimal: q1 18 + r1 15 + w1 12 + r2 10 (flex). Set: w2 8 in the flex.
    assert!((m.my_projected - 55.0).abs() < 1e-9, "{}", m.my_projected);
    assert!((m.set_projected - 53.0).abs() < 1e-9, "{}", m.set_projected);
    assert!((m.opp_projected - 51.0).abs() < 1e-9, "{}", m.opp_projected);
    assert!(m.set_projected <= m.my_projected);
    assert_eq!(m.rows.len(), 4, "one row per starting slot");
    assert_eq!(m.set_rows.len(), 4);

    for row in m.rows.iter().chain(m.set_rows.iter()) {
        assert!(
            (row.margin - (row.my_points - row.opp_points)).abs() < 1e-9,
            "margin must be my points minus theirs in {}",
            row.slot
        );
    }
    let my_sum: f64 = m.rows.iter().map(|r| r.my_points).sum();
    let opp_sum: f64 = m.rows.iter().map(|r| r.opp_points).sum();
    assert!((my_sum - m.my_projected).abs() < 1e-9);
    assert!((opp_sum - m.opp_projected).abs() < 1e-9);
    let margin_sum: f64 = m.set_rows.iter().map(|r| r.margin).sum();
    assert!((margin_sum - (m.set_projected - m.opp_projected)).abs() < 1e-9);
    // My roster appears in my column of the set rows.
    assert!(m.set_rows.iter().any(|r| r.my_name == "Ace Passer"));
    assert!(m.set_rows.iter().any(|r| r.opp_name == "Rival Passer"));
}

/// The bench back does outscore the flex; put both on Tampa Bay, whose game
/// kicked off in the fixture's second quarter. Sleeper will not accept that
/// swap any more, so the screen must not keep offering it — nor keep counting
/// its two points as "on the table" above a panel with nothing in it.
#[test]
fn a_swap_between_two_players_already_on_the_field_is_not_offered() {
    let (mut loaded, season, config) = common::fixture();
    // The board is shared behind an `Arc` so the poll tick can copy the loaded
    // league without duplicating it; a test that edits it takes its own copy.
    for player in std::sync::Arc::make_mut(&mut loaded.board).iter_mut() {
        if player.player_id == "r2" || player.player_id == "w2" {
            player.team = Some("TB".to_string());
        }
    }
    let v = build_season_view(&loaded, &season, config.my_user_id.as_deref());
    assert!(v.calls.is_empty(), "{:?}", v.calls);
    assert_eq!(
        v.points_on_table, 0.0,
        "points nobody can move are not on the table"
    );
}

/// The same fixture with the scoreboard cleared: nothing has kicked off, so
/// the call the projections found stands, with its reason and its gain.
#[test]
fn the_bench_back_outscoring_the_flex_is_exactly_one_call_before_kickoff() {
    let (loaded, mut season, config) = common::fixture();
    season.scores = std::sync::Arc::new(Vec::new());
    let v = build_season_view(&loaded, &season, config.my_user_id.as_deref());

    assert_eq!(v.calls.len(), 1, "{:?}", v.calls);
    let call = &v.calls[0];
    assert_eq!(call.slot, "FLEX");
    assert_eq!(call.player_in, "Bench Back");
    assert_eq!(call.player_in_id, "r2");
    assert_eq!(call.player_out, "Slot Wideout");
    assert_eq!(call.player_out_id, "w2");
    assert!((call.gain - 2.0).abs() < 1e-9, "{}", call.gain);
    assert!(
        call.why.contains("projects 10.0 against 8.0"),
        "{}",
        call.why
    );
    assert!((v.points_on_table - 2.0).abs() < 1e-9);
}

#[test]
fn roster_rows_are_grouped_start_bye_bench_and_sorted_by_points() {
    let v = view();
    let roles: Vec<(&str, &str)> = v
        .roster
        .iter()
        .map(|r| (r.player_id.as_str(), r.role.as_str()))
        .collect();
    assert_eq!(
        roles,
        vec![
            ("q1", "Start"),
            ("r1", "Start"),
            ("w1", "Start"),
            ("w2", "Start"),
            ("w5", "Bye"),
            ("r2", "Bench"),
        ]
    );
    let q1 = &v.roster[0];
    assert_eq!(q1.name, "Ace Passer");
    assert_eq!(q1.position, "QB");
    assert_eq!(q1.team.as_deref(), Some("ATL"));
    assert!((q1.points - 40.0).abs() < 1e-9, "season-to-date points");
    assert!((q1.projected - 18.0).abs() < 1e-9, "this week's projection");
    let bye = v.roster.iter().find(|r| r.player_id == "w5").unwrap();
    assert_eq!(bye.projected, 0.0, "a bye week projects nothing");
}

#[test]
fn standings_seed_by_record_and_their_odds_sum_to_the_bracket_size() {
    let v = view();
    assert_eq!(v.standings.len(), 4);
    assert_eq!(v.standings[0].roster_id, 1, "2-0 seeds first");
    assert_eq!(v.standings[0].seed, 1);
    assert_eq!(v.standings[0].record, "2\u{2013}0");
    assert!(v.standings[0].is_mine);
    assert_eq!(v.standings[3].roster_id, 4, "0-2 seeds last");
    assert!(v.standings.iter().skip(1).all(|s| !s.is_mine));
    for row in &v.standings {
        assert!((0.0..=1.0).contains(&row.playoff_odds), "{row:?}");
        assert!(
            row.projected_points >= row.points_for,
            "banked points plus projections can only grow"
        );
    }
    let odds_sum: f64 = v.standings.iter().map(|s| s.playoff_odds).sum();
    assert!(
        (odds_sum - 2.0).abs() < 1e-9,
        "every simulation admits exactly playoff_teams teams, got {odds_sum}"
    );
    assert!(
        (v.header.playoff_odds - v.standings[0].playoff_odds).abs() < 1e-9,
        "the header repeats my standings row's odds"
    );
}

#[test]
fn header_reflects_the_projection_gap_and_the_next_kickoff() {
    let (loaded, season, config) = common::fixture();
    let v = build_season_view(&loaded, &season, config.my_user_id.as_deref());
    assert_eq!(v.header.opponent_name.as_deref(), Some("User Two"));
    assert!((v.header.my_projected - 55.0).abs() < 1e-9);
    assert!((v.header.my_set_projected - 53.0).abs() < 1e-9);
    assert!((v.header.opp_projected - 51.0).abs() < 1e-9);
    assert!(
        v.header.win_odds_best > 0.5 && v.header.win_odds_best < 1.0,
        "projected ahead 55-51 means favoured but not certain: {}",
        v.header.win_odds_best
    );
    let upcoming = season.scores[1].start_time;
    assert_eq!(v.header.locks_in_ms, upcoming);
    assert_eq!(v.live.next_kickoff_ms, upcoming);
    assert_eq!(v.data_health.fetched_at, season.fetched_at);
    assert!(v.generated_at >= season.fetched_at);
}

#[test]
fn a_suboptimal_set_lineup_is_given_worse_odds_than_the_best_one() {
    // The fixture leaves a 10-point back on the bench behind an 8-point
    // receiver, so best projects 55 and set projects 53 against the same
    // opponent's 51. The screen offers to show either lineup; if both were
    // quoted the one win probability, it would be telling the manager he is
    // 74% to win while also telling him he is starting the wrong player.
    let v = view();
    assert!(v.header.my_set_projected < v.header.my_projected);
    assert!(
        v.header.win_odds_set < v.header.win_odds_best,
        "points left on the bench have to cost win probability: set {} vs best {}",
        v.header.win_odds_set,
        v.header.win_odds_best
    );
    // Still the favourite at 53-51 — worse odds, not a different matchup.
    assert!(v.header.win_odds_set > 0.5 && v.header.win_odds_set < 1.0);
}

#[test]
fn an_already_optimal_lineup_is_priced_the_same_either_way() {
    let (loaded, mut season, config) = common::fixture();
    // Start the bench back in the flex: now the set lineup is the best one.
    for matchup in std::sync::Arc::make_mut(&mut season.matchups) {
        if matchup.roster_id == 1 {
            let starters = matchup.starters.as_mut().expect("fixture sets starters");
            for id in starters.iter_mut() {
                if id == "w2" {
                    *id = "r2".to_string();
                }
            }
        }
    }
    let v = build_season_view(&loaded, &season, config.my_user_id.as_deref());
    assert!((v.header.my_set_projected - v.header.my_projected).abs() < 1e-9);
    assert!(
        (v.header.win_odds_set - v.header.win_odds_best).abs() < 1e-9,
        "one lineup, one number"
    );
    assert!(v.calls.is_empty(), "nothing left to change");
}

#[test]
fn the_free_agent_back_tops_waivers_with_a_bid_inside_my_budget() {
    let v = view();
    assert_eq!(v.waiver_budget_total, Some(100.0));
    assert_eq!(v.waiver_budget_left, Some(75.0), "100 minus 25 spent");
    assert!(!v.waivers.is_empty(), "fa1 improves my flex");
    let top = &v.waivers[0];
    assert_eq!(top.player_id, "fa1");
    assert_eq!(top.name, "Waiver Back");
    assert_eq!(top.position, "RB");
    // Optimal lineup goes 55 -> 58 with a 13-point RB in the flex.
    assert!((top.gain_points - 3.0).abs() < 1e-9, "{}", top.gain_points);
    let bid = top.suggested_bid.expect("FAAB league suggests a bid");
    assert!(bid >= 1 && f64::from(bid) <= 75.0 * 0.5, "bid {bid} capped");
    assert!(
        v.waivers.iter().all(|w| w.player_id != "fa2"),
        "a zero-projection free agent is never a target"
    );
    assert!(
        v.waivers.iter().all(|w| w.player_id != "r2"),
        "rostered players are not free agents"
    );
}

#[test]
fn team_avatars_only_cover_managers_who_set_one() {
    let v = view();
    assert_eq!(v.team_avatars.len(), 2);
    assert_eq!(
        v.team_avatars.get(&1).map(String::as_str),
        Some("avatar-one")
    );
    assert_eq!(
        v.team_avatars.get(&2).map(String::as_str),
        Some("avatar-two")
    );
    assert!(!v.team_avatars.contains_key(&3));
}

#[test]
fn last_season_rows_pass_through_untouched() {
    let v = view();
    assert_eq!(v.last_season.len(), 1);
    assert_eq!(v.last_season[0].name, "User Two");
    assert_eq!(v.last_season[0].tag.as_deref(), Some("Champ"));
}

#[test]
fn a_cached_analysis_is_reused_verbatim_and_the_live_slice_is_not() {
    use draft_assistant_lib::season::{build_season_view_cached, SeasonAnalysis};

    let (loaded, season, config) = common::fixture();
    let full = build_season_view_cached(&loaded, &season, config.my_user_id.as_deref(), None);

    // Hand back a deliberately wrong analysis: if the poll path recomputed it,
    // these values would be replaced rather than passed through.
    let planted = SeasonAnalysis {
        standings: Vec::new(),
        waivers: Vec::new(),
        trades: Vec::new(),
        activity: Vec::new(),
        recent_trades: Vec::new(),
        trends: Default::default(),
        as_of: 1_700_000_000,
    };
    let cheap = build_season_view_cached(
        &loaded,
        &season,
        config.my_user_id.as_deref(),
        Some(&planted),
    );
    assert!(cheap.standings.is_empty(), "standings were recomputed");
    assert!(cheap.waivers.is_empty(), "waivers were recomputed");
    assert!(cheap.trades.is_empty(), "trades were recomputed");
    assert!(
        cheap.recent_trades.is_empty(),
        "completed trades were recomputed"
    );
    assert!(cheap.trends.series.is_empty(), "trends were recomputed");

    // Everything outside the analysis is still built fresh.
    assert_eq!(cheap.week, full.week);
    assert_eq!(cheap.my_roster_id, full.my_roster_id);
    assert_eq!(cheap.roster.len(), full.roster.len());
    assert_eq!(
        cheap.live.totals.my_live_points,
        full.live.totals.my_live_points
    );

    // And lifting the analysis back out of a full view round-trips.
    let lifted = SeasonAnalysis::of(&full);
    let reused = build_season_view_cached(
        &loaded,
        &season,
        config.my_user_id.as_deref(),
        Some(&lifted),
    );
    assert_eq!(reused.standings.len(), full.standings.len());
    assert_eq!(reused.waivers.len(), full.waivers.len());
    assert_eq!(reused.trades.len(), full.trades.len());
    assert_eq!(reused.activity.len(), full.activity.len());
    assert_eq!(reused.recent_trades.len(), full.recent_trades.len());
    assert_eq!(reused.trends.series.len(), full.trends.series.len());
}

#[test]
fn the_feeds_come_from_the_cache_but_the_empty_slots_do_not() {
    use draft_assistant_lib::season::{build_season_view_cached, SeasonAnalysis};

    let (loaded, season, config) = common::fixture();
    let full = build_season_view_cached(&loaded, &season, config.my_user_id.as_deref(), None);
    // The fixture's feed leads with a lineup gap, then transactions.
    assert_eq!(full.activity[0].kind, "Lineup");
    assert!(full.activity.len() > 1, "the fixture has transaction items");
    assert!(!full.recent_trades.is_empty());
    assert!(!full.trends.series.is_empty());

    // Mark every cached feed so anything recomputed is obvious.
    let mut planted = SeasonAnalysis::of(&full);
    assert!(
        planted.activity.iter().all(|i| i.kind != "Lineup"),
        "gaps are read off live rosters, so they must not be carried"
    );
    for item in &mut planted.activity {
        item.text = "carried over".to_string();
    }
    for trade in &mut planted.recent_trades {
        trade.transaction_id = "carried over".to_string();
    }
    planted.trends.series[0].name = "carried over".to_string();

    let reused = build_season_view_cached(
        &loaded,
        &season,
        config.my_user_id.as_deref(),
        Some(&planted),
    );

    // The transaction half of the feed, the completed trades and the whole
    // trends panel are the cached copies — none of them was rebuilt.
    assert!(
        reused.activity[1..]
            .iter()
            .all(|i| i.text == "carried over"),
        "the transaction feed was recomputed"
    );
    assert!(
        reused
            .recent_trades
            .iter()
            .all(|t| t.transaction_id == "carried over"),
        "completed trades were recomputed"
    );
    assert_eq!(reused.trends.series[0].name, "carried over");

    // The empty-starter-slot items still describe the rosters as they are now.
    assert_eq!(reused.activity[0].kind, "Lineup");
    assert_eq!(reused.activity[0].text, full.activity[0].text);
    assert_eq!(reused.activity.len(), full.activity.len());
}

#[test]
fn reused_analysis_reports_when_it_was_actually_built() {
    use draft_assistant_lib::season::{build_season_view_cached, SeasonAnalysis};

    let (loaded, season, config) = common::fixture();
    let fresh = build_season_view_cached(&loaded, &season, config.my_user_id.as_deref(), None);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!(
        fresh.analysis_as_of_secs.abs_diff(now) < 5,
        "a fresh build is stamped now, got {}",
        fresh.analysis_as_of_secs
    );

    // A view built from a ten-minute-old analysis must say ten minutes old,
    // not "just now" — that is the whole point of carrying the stamp.
    let stale = SeasonAnalysis {
        as_of: now - 600,
        ..SeasonAnalysis::of(&fresh)
    };
    let reused =
        build_season_view_cached(&loaded, &season, config.my_user_id.as_deref(), Some(&stale));
    assert_eq!(reused.analysis_as_of_secs, now - 600);

    // Dropping the cache rebuilds it, and the stamp moves back to now.
    let rebuilt = build_season_view_cached(&loaded, &season, config.my_user_id.as_deref(), None);
    assert!(rebuilt.analysis_as_of_secs >= fresh.analysis_as_of_secs);
    assert!(rebuilt.analysis_as_of_secs > reused.analysis_as_of_secs);
}

/// The odds used to be priced off projections alone, so they read the same at
/// midnight Sunday as they did on Saturday morning. The fixture's Tampa Bay
/// game is in its second quarter with real points on the board, and that has
/// to be worth something.
#[test]
fn live_scoring_moves_the_win_odds_away_from_the_pregame_reading() {
    let (loaded, mut season, config) = common::fixture();
    let live = build_season_view(&loaded, &season, config.my_user_id.as_deref());
    season.scores = std::sync::Arc::new(Vec::new());
    let pregame = build_season_view(&loaded, &season, config.my_user_id.as_deref());

    // The projections themselves are untouched: only the pricing has moved.
    assert!((live.header.my_projected - pregame.header.my_projected).abs() < 1e-9);
    assert!((live.header.opp_projected - pregame.header.opp_projected).abs() < 1e-9);
    assert!(
        (live.header.win_odds_set - pregame.header.win_odds_set).abs() > 1e-6,
        "live {} vs pregame {}",
        live.header.win_odds_set,
        pregame.header.win_odds_set
    );
}

/// And the other half of the promise: with an empty scoreboard the odds are
/// exactly what the projection-only model always gave.
#[test]
fn with_nothing_kicked_off_the_odds_are_the_pregame_ones() {
    let (loaded, mut season, config) = common::fixture();
    season.scores = std::sync::Arc::new(Vec::new());
    let a = build_season_view(&loaded, &season, config.my_user_id.as_deref());
    let b = build_season_view(&loaded, &season, config.my_user_id.as_deref());
    assert_eq!(a.header.win_odds_set, b.header.win_odds_set);
    assert!(
        a.header.win_odds_set > 0.5,
        "the fixture's better roster is favoured: {}",
        a.header.win_odds_set
    );
}
