use draft_assistant_lib::board::build_board;
use draft_assistant_lib::roster::RosterRules;
use draft_assistant_lib::sleeper::{Draft, League, PlayerMeta, ProjectionRow};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

#[derive(Deserialize)]
struct BoardFixture {
    league: League,
    draft: Draft,
    season_rows: Vec<ProjectionRow>,
}

#[test]
fn sleeper_shaped_fixture_builds_mixed_board_with_kicker() {
    let fixture: BoardFixture = serde_json::from_str(include_str!("fixtures/board_input.json"))
        .expect("sanitized Sleeper fixture must deserialize");
    let rules = RosterRules::new(&fixture.league.roster_positions);
    let player_meta: HashMap<String, PlayerMeta> = fixture
        .season_rows
        .iter()
        .filter_map(|row| {
            row.player
                .clone()
                .map(|player| (row.player_id.clone(), player))
        })
        .collect();
    let mut warnings = Vec::new();

    let result = build_board(
        &fixture.league,
        &fixture.draft,
        &player_meta,
        &fixture.season_rows,
        &[],
        &rules,
        &mut warnings,
    );

    let positions: HashSet<&str> = result
        .players
        .iter()
        .map(|player| player.position.as_str())
        .collect();
    assert_eq!(
        positions,
        HashSet::from(["QB", "RB", "WR", "TE", "K", "DEF"])
    );
    assert!(result.players.iter().all(|player| player.points >= 20.0));
    assert!(result.replacement.demand.contains_key("K"));
    assert!(result.replacement.baseline.contains_key("QB"));
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// One weekly projection row: `opponent` is `None` on the bye week.
fn weekly(
    player_id: &str,
    team: &str,
    week: u32,
    opponent: Option<&str>,
    rush_yd: f64,
) -> ProjectionRow {
    ProjectionRow {
        player_id: player_id.into(),
        stats: Some(HashMap::from([("rush_yd".to_string(), rush_yd)])),
        player: Some(PlayerMeta {
            full_name: None,
            first_name: None,
            last_name: None,
            position: None,
            team: Some(team.into()),
            fantasy_positions: None,
            injury_status: None,
            age: None,
            years_exp: None,
        }),
        week: Some(week),
        opponent: opponent.map(str::to_string),
    }
}

fn fixture() -> BoardFixture {
    serde_json::from_str(include_str!("fixtures/board_input.json")).unwrap()
}

fn meta_of(fixture: &BoardFixture) -> HashMap<String, PlayerMeta> {
    fixture
        .season_rows
        .iter()
        .filter_map(|row| row.player.clone().map(|p| (row.player_id.clone(), p)))
        .collect()
}

#[test]
fn bye_weeks_come_from_the_week_with_no_opponents_and_survive_a_stray_row() {
    let fixture = fixture();
    let rules = RosterRules::new(&fixture.league.roster_positions);
    let mut weekly_rows = Vec::new();
    // Team BBB (rb-1): a full slate except week 7, the bye.
    for week in 1..=18 {
        let opp = if week == 7 { None } else { Some("XXX") };
        weekly_rows.push(weekly("rb-1", "BBB", week, opp, 60.0));
    }
    // Team AAA: four players, bye week 9 — but one stale row for a traded
    // player still lists an opponent that week. One row against four must
    // not move the bye.
    for player in ["qb-1", "aaa-2", "aaa-3", "aaa-4"] {
        for week in 1..=18 {
            let opp = if week == 9 { None } else { Some("YYY") };
            weekly_rows.push(weekly(player, "AAA", week, opp, 0.0));
        }
    }
    weekly_rows.push(weekly("aaa-5", "AAA", 9, Some("ZZZ"), 0.0));
    // Team CCC (wr-1): no weekly rows at all.
    let mut warnings = Vec::new();
    let result = build_board(
        &fixture.league,
        &fixture.draft,
        &meta_of(&fixture),
        &fixture.season_rows,
        &weekly_rows,
        &rules,
        &mut warnings,
    );
    let bye = |id: &str| {
        result
            .players
            .iter()
            .find(|p| p.player_id == id)
            .unwrap()
            .bye_week
    };
    assert_eq!(bye("rb-1"), Some(7));
    assert_eq!(
        bye("qb-1"),
        Some(9),
        "one stray row does not poison the team"
    );
    assert_eq!(bye("wr-1"), None, "no weekly rows, no bye claimed");
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn per_game_bonuses_come_from_weekly_means_and_only_when_the_league_pays_them() {
    let mut fixture = fixture();
    let rules = RosterRules::new(&fixture.league.roster_positions);
    let weekly_rows: Vec<ProjectionRow> = (1..=17)
        .map(|week| weekly("rb-1", "BBB", week, Some("XXX"), 110.0))
        .collect();
    let build = |fixture: &BoardFixture, warnings: &mut Vec<String>| {
        build_board(
            &fixture.league,
            &fixture.draft,
            &meta_of(fixture),
            &fixture.season_rows,
            &weekly_rows,
            &rules,
            warnings,
        )
    };
    let rb = |result: &draft_assistant_lib::board::BoardBuild| {
        result
            .players
            .iter()
            .find(|p| p.player_id == "rb-1")
            .cloned()
            .unwrap()
    };
    let mut warnings = Vec::new();
    let without = rb(&build(&fixture, &mut warnings));
    assert_eq!(
        without.bonus_points, 0.0,
        "the fixture league pays no bonuses"
    );

    fixture
        .league
        .scoring_settings
        .insert("bonus_rush_yd_100".into(), 3.0);
    let with = rb(&build(&fixture, &mut warnings));
    // 17 games projected at 110 rushing yards: most clear 100, none are sure.
    assert!(
        with.bonus_points > 17.0 && with.bonus_points < 51.0,
        "{}",
        with.bonus_points
    );
    assert!((with.points - without.points - with.bonus_points).abs() < 1e-9);
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn a_projections_feed_with_no_usable_rows_is_a_warning_and_an_empty_board() {
    let fixture = fixture();
    let rules = RosterRules::new(&fixture.league.roster_positions);
    let unusable: Vec<ProjectionRow> = fixture
        .season_rows
        .iter()
        .map(|row| ProjectionRow {
            stats: None,
            ..row.clone()
        })
        .collect();
    let mut warnings = Vec::new();
    let result = build_board(
        &fixture.league,
        &fixture.draft,
        &meta_of(&fixture),
        &unusable,
        &[],
        &rules,
        &mut warnings,
    );
    assert!(result.players.is_empty());
    assert_eq!(
        warnings,
        vec!["no scored players — projections fetch likely failed".to_string()]
    );
}
