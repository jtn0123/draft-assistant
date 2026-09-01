//! The rest of `build_season_view`: live scoreboard, feeds (activity, trades,
//! trends), and the degraded paths — no identified user, a bye week, and a
//! week with no matchup rows at all.

mod common;

use draft_assistant_lib::season::build_season_view;

#[test]
fn live_section_joins_both_lineups_to_their_nfl_games() {
    let (loaded, season, config) = common::fixture();
    let v = build_season_view(&loaded, &season, config.my_user_id.as_deref());

    assert_eq!(v.live.games.len(), 2, "both games contain tracked players");
    let live_game = &v.live.games[0];
    assert_eq!(live_game.game_id, "g-live");
    assert_eq!(live_game.home, "ATL");
    assert!(live_game.status.starts_with("Q2"), "{}", live_game.status);
    assert_eq!(
        live_game.my_starter_count(),
        4,
        "my whole lineup plays here"
    );
    assert!(
        live_game.chips.iter().all(|c| c.is_mine),
        "nobody from the opposing lineup is in the ATL-TB game"
    );
    // players_points wins over the projection for a player mid-game.
    let q1 = live_game
        .chips
        .iter()
        .find(|c| c.player_id == "q1")
        .unwrap();
    assert!((q1.points - 21.5).abs() < 1e-9);
    assert_eq!(q1.slot, "QB");

    let pre_game = &v.live.games[1];
    assert_eq!(pre_game.game_id, "g-pre");
    assert_eq!(pre_game.channel.as_deref(), Some("CBS"));
    assert!(pre_game.chips.iter().all(|c| !c.is_mine));
    assert_eq!(pre_game.chips.len(), 4, "all four rival starters");

    assert_eq!(v.live.totals.my_playing, 4);
    assert_eq!(v.live.totals.my_pre, 0);
    assert!((v.live.totals.my_live_points - 56.5).abs() < 1e-9);
    assert_eq!(v.live.totals.opp_live_points, 0.0, "their game is pregame");
    assert_eq!(v.live.windows.len(), 2);
    assert_eq!(v.live.next_kickoff_ms, season.scores[1].start_time);
    assert!(v.live.bye_teams.contains(&"KC".to_string()));
    assert!(!v.live.bye_teams.contains(&"ATL".to_string()));
}

#[test]
fn activity_leads_with_lineup_gaps_then_newest_transactions() {
    let (loaded, season, config) = common::fixture();
    let v = build_season_view(&loaded, &season, config.my_user_id.as_deref());

    let kinds: Vec<&str> = v.activity.iter().map(|a| a.kind.as_str()).collect();
    assert_eq!(kinds, vec!["Lineup", "Trade", "Waiver"]);
    assert_eq!(
        v.activity[0].text, "User Three has an empty FLEX slot",
        "roster 3 left its flex unfilled"
    );
    assert_eq!(v.activity[0].roster_id, Some(3));
    assert_eq!(
        v.activity[1].text,
        "User One gets Rival Wideout \u{b7} User Two gets Alpha Wideout, 2027 2nd"
    );
    assert_eq!(v.activity[2].text, "User Two claimed Roster Filler for $12");
    let faces = &v.activity[2].players;
    assert_eq!(faces.len(), 1);
    assert_eq!(faces[0].id, "fa2");
    // The face carries who he is, so the row can caption him and fall back to
    // his team's mark when Sleeper has no photo of him.
    assert_eq!(faces[0].name, "Roster Filler");
    assert_eq!(faces[0].team.as_deref(), Some("LV"));
}

#[test]
fn recent_trades_name_both_sides_and_flag_mine() {
    let (loaded, season, config) = common::fixture();
    let v = build_season_view(&loaded, &season, config.my_user_id.as_deref());
    assert_eq!(v.recent_trades.len(), 1);
    let deal = &v.recent_trades[0];
    assert_eq!(deal.transaction_id, "trade-1");
    assert!(deal.involves_me);
    assert!(!deal.pending);
    assert_eq!(deal.sides.len(), 2);
    assert_eq!(deal.sides[0].team, "User One");
    assert_eq!(deal.sides[0].gets, vec!["Rival Wideout".to_string()]);
    assert_eq!(deal.sides[1].team, "User Two");
    assert_eq!(
        deal.sides[1].gets,
        vec!["Alpha Wideout".to_string(), "2027 2nd".to_string()],
        "a traded pick is named by year and round, not filed under \"draft picks\""
    );
}

#[test]
fn trade_ideas_only_propose_deals_that_help_both_rosters() {
    let (loaded, season, config) = common::fixture();
    let v = build_season_view(&loaded, &season, config.my_user_id.as_deref());
    for idea in &v.trades {
        assert_ne!(idea.roster_id, 1, "never trades with myself");
        assert!(idea.my_edge > 0.0, "{idea:?}");
        assert!(idea.their_edge > 0.0, "{idea:?}");
        assert!(!idea.get_name.is_empty() && !idea.give_name.is_empty());
    }
}

#[test]
fn trends_graph_every_team_and_explains_the_moves() {
    let (loaded, season, config) = common::fixture();
    let v = build_season_view(&loaded, &season, config.my_user_id.as_deref());

    assert_eq!(v.trends.series.len(), 4);
    let strongest = &v.trends.series[0];
    assert_eq!(strongest.roster_id, 1, "strongest-today leads the legend");
    assert!(strongest.is_mine);
    assert_eq!(strongest.points.len(), 2);
    assert!((strongest.points[1].strength - 55.0).abs() < 1e-9);

    assert_eq!(v.trends.changes.len(), 2, "{:?}", v.trends.changes);
    let mine = &v.trends.changes[0];
    assert_eq!(mine.roster_id, 1, "biggest swing first");
    assert!(mine.is_mine);
    assert!((mine.delta - 5.0).abs() < 1e-9);
    assert_eq!(mine.at, season.history.snapshots[1].taken_at);
    assert_eq!(
        mine.reasons,
        vec!["Bench Back projection +5.0/wk".to_string()]
    );
    let theirs = &v.trends.changes[1];
    assert_eq!(theirs.roster_id, 2);
    assert_eq!(
        theirs.reasons,
        vec!["claimed Roster Filler for $12".to_string()]
    );
}

#[test]
fn an_unidentified_user_still_gets_league_wide_panels() {
    let (loaded, season, _) = common::fixture();
    let v = build_season_view(&loaded, &season, None);

    assert_eq!(v.my_roster_id, None);
    assert!(v.matchup.is_none());
    assert!(v.calls.is_empty());
    assert!(v.roster.is_empty());
    assert_eq!(v.header.opponent_name, None);
    assert_eq!(v.header.my_projected, 0.0);
    assert_eq!(v.header.playoff_odds, 0.0);
    assert_eq!(v.waiver_budget_left, None, "no roster, no budget");
    assert_eq!(v.waiver_budget_total, Some(100.0));
    assert_eq!(v.standings.len(), 4);
    assert!(v.standings.iter().all(|s| !s.is_mine));
    // The f64 additive identity is -0.0; the view must not serialise that.
    assert_eq!(v.points_on_table, 0.0);
    assert!(
        v.points_on_table.is_sign_positive(),
        "an empty sum must normalise -0.0 away"
    );
}

#[test]
fn a_bye_week_matchup_faces_an_empty_opponent() {
    let (loaded, mut season, config) = common::fixture();
    // Only my row remains and it belongs to no game.
    season.matchups.truncate(1);
    season.matchups[0].matchup_id = None;
    let v = build_season_view(&loaded, &season, config.my_user_id.as_deref());

    let m = v.matchup.expect("my matchup row still exists");
    assert_eq!(m.opp_name, "Bye week");
    assert_eq!(m.opp_avatar, None);
    assert_eq!(m.opp_projected, 0.0);
    assert!(m.rows.iter().all(|r| r.opp_name.is_empty()));
    assert!((m.my_projected - 55.0).abs() < 1e-9);
    assert_eq!(v.header.opponent_name, None);
    assert!(
        v.header.win_odds_best > 0.9 && v.header.win_odds_set > 0.9,
        "55 projected against nothing, either lineup: {} / {}",
        v.header.win_odds_best,
        v.header.win_odds_set
    );
}

#[test]
fn with_no_matchups_the_roster_assumes_the_optimal_lineup() {
    let (loaded, mut season, config) = common::fixture();
    season.matchups.clear();
    let v = build_season_view(&loaded, &season, config.my_user_id.as_deref());

    assert!(v.matchup.is_none());
    assert!(v.calls.is_empty(), "optimal vs optimal has no diff");
    let role_of = |id: &str| {
        v.roster
            .iter()
            .find(|r| r.player_id == id)
            .map(|r| r.role.clone())
            .unwrap()
    };
    assert_eq!(role_of("r2"), "Start", "the optimal flex is r2");
    assert_eq!(role_of("w2"), "Bench");
    assert!(
        v.live.games.is_empty(),
        "nobody is tracked without a matchup"
    );
}

#[test]
fn a_team_without_an_owner_gets_a_fallback_name() {
    let (loaded, mut season, config) = common::fixture();
    season.rosters[1].owner_id = None;
    let v = build_season_view(&loaded, &season, config.my_user_id.as_deref());
    let m = v.matchup.expect("matchup still pairs rosters 1 and 2");
    assert_eq!(m.opp_name, "Team 2");
    assert_eq!(m.opp_avatar, None, "no owner, no avatar");
    assert!(v.standings.iter().any(|s| s.name == "Team 2"));
}
