//! What the board reads off a Sleeper projection row, and what it makes of it:
//! which ADP column belongs to this league, where kickers and defences sort,
//! how a defence gets its name, and how a bye week is inferred from weekly
//! coverage. Each of these has been wrong in a way that only showed up on the
//! screen, so each is pinned here against rows shaped like the real ones.

use draft_assistant_lib::board::{adp_key, build_board, BoardBuild, ONESIE_RANK_DISCOUNT};
use draft_assistant_lib::roster::RosterRules;
use draft_assistant_lib::sleeper::{Draft, League, PlayerMeta, ProjectionRow};
use std::collections::HashMap;

fn league(roster_positions: &[&str], rec: Option<f64>) -> League {
    let mut scoring_settings: HashMap<String, f64> = HashMap::new();
    scoring_settings.insert("rec_yd".into(), 0.1);
    scoring_settings.insert("rec_td".into(), 6.0);
    scoring_settings.insert("rush_yd".into(), 0.1);
    scoring_settings.insert("pass_yd".into(), 0.04);
    scoring_settings.insert("sack".into(), 1.0);
    if let Some(rec) = rec {
        scoring_settings.insert("rec".into(), rec);
    }
    League {
        league_id: "l".into(),
        name: "L".into(),
        season: "2026".into(),
        status: "pre_draft".into(),
        total_rosters: 12,
        roster_positions: roster_positions.iter().map(|s| (*s).to_string()).collect(),
        scoring_settings,
        draft_id: Some("d".into()),
        previous_league_id: None,
        settings: Default::default(),
    }
}

fn draft() -> Draft {
    serde_json::from_value(serde_json::json!({
        "draft_id": "d", "status": "pre_draft", "type": "snake",
        "settings": {"teams": 12, "rounds": 15}
    }))
    .expect("draft fixture")
}

fn row(player_id: &str, position: &str, stats: serde_json::Value) -> ProjectionRow {
    serde_json::from_value(serde_json::json!({
        "player_id": player_id,
        "stats": stats,
        "player": {"full_name": format!("{player_id} name"), "position": position, "team": "AAA"},
    }))
    .expect("season row")
}

fn build(league: &League, rows: &[ProjectionRow], weekly: &[ProjectionRow]) -> BoardBuild {
    let rules = RosterRules::new(&league.roster_positions);
    let mut warnings = Vec::new();
    build_board(
        league,
        &draft(),
        &HashMap::new(),
        rows,
        weekly,
        &rules,
        &mut warnings,
    )
}

const STANDARD: &[&str] = &["QB", "RB", "WR", "TE", "FLEX", "K", "DEF", "BN"];

#[test]
fn the_adp_column_is_the_one_this_league_drafts_on() {
    assert_eq!(adp_key(&league(STANDARD, Some(1.0))), "adp_ppr");
    assert_eq!(adp_key(&league(STANDARD, Some(0.5))), "adp_half_ppr");
    assert_eq!(adp_key(&league(STANDARD, Some(0.0))), "adp_std");
    // A league that does not list `rec` at all scores nothing per catch.
    assert_eq!(adp_key(&league(STANDARD, None)), "adp_std");
    // Two quarterbacks start, whichever way the league spells it.
    let superflex = ["QB", "RB", "WR", "TE", "SUPER_FLEX", "DEF", "BN"];
    assert_eq!(adp_key(&league(&superflex, Some(1.0))), "adp_2qb");
    let two_qb = ["QB", "QB", "RB", "WR", "TE", "DEF", "BN"];
    assert_eq!(adp_key(&league(&two_qb, Some(1.0))), "adp_2qb");
}

#[test]
fn a_half_ppr_board_reads_the_half_ppr_adp() {
    let rows = [row(
        "wr-1",
        "WR",
        serde_json::json!({"rec_yd": 1200.0, "rec_td": 8.0,
            "adp_ppr": 12.0, "adp_half_ppr": 30.0, "adp_std": 55.0}),
    )];
    let half = build(&league(STANDARD, Some(0.5)), &rows, &[]);
    assert_eq!(half.players[0].adp, Some(30.0));
    let full = build(&league(STANDARD, Some(1.0)), &rows, &[]);
    assert_eq!(full.players[0].adp, Some(12.0));
    let standard = build(&league(STANDARD, Some(0.0)), &rows, &[]);
    assert_eq!(standard.players[0].adp, Some(55.0));
}

#[test]
fn a_missing_column_falls_back_to_ppr_rather_than_losing_the_adp() {
    // Sleeper publishes four ADP columns but not for every player: a row with
    // only the PPR one must still carry an ADP in a half-PPR league.
    let rows = [row(
        "wr-1",
        "WR",
        serde_json::json!({"rec_yd": 1200.0, "rec_td": 8.0, "adp_ppr": 12.0}),
    )];
    let built = build(&league(STANDARD, Some(0.5)), &rows, &[]);
    assert_eq!(built.players[0].adp, Some(12.0));
}

#[test]
fn a_defence_outranks_nobody_it_cannot_be_taken_ahead_of() {
    // The best defence out-VORPs the receiver, but a defence's value is there
    // for anyone in the last two rounds and his is not, so he ranks ahead of
    // it. On raw VORP this league's best defence sat at overall 64.
    let mut rows = Vec::new();
    for (i, sacks) in [50.0, 30.0, 28.0, 26.0].iter().enumerate() {
        rows.push(row(
            &format!("def-{i}"),
            "DEF",
            serde_json::json!({"sack": sacks, "adp_ppr": 90.0 + i as f64}),
        ));
    }
    for (i, yards) in [900.0, 850.0, 800.0, 780.0, 760.0, 740.0]
        .iter()
        .enumerate()
    {
        rows.push(row(
            &format!("wr-{i}"),
            "WR",
            serde_json::json!({"rec_yd": yards, "adp_ppr": 20.0 + i as f64}),
        ));
    }
    let mut league = league(STANDARD, Some(0.5));
    league.total_rosters = 2;
    let built = build(&league, &rows, &[]);
    let by_id: HashMap<&str, &draft_assistant_lib::board::BoardPlayer> = built
        .players
        .iter()
        .map(|p| (p.player_id.as_str(), p))
        .collect();
    let def = by_id["def-0"];
    let wr = by_id["wr-0"];
    assert!(
        def.vorp > wr.vorp && def.vorp - wr.vorp < ONESIE_RANK_DISCOUNT,
        "the fixture needs the DEF ahead on VORP by less than the discount:          {} vs {}",
        def.vorp,
        wr.vorp
    );
    assert!(
        def.overall_rank > wr.overall_rank,
        "DEF ranked {} ahead of WR {}",
        def.overall_rank,
        wr.overall_rank
    );
}

#[test]
fn a_defence_with_no_embedded_meta_still_gets_a_name() {
    // The row carries no `player` block at all; the dictionary has the name.
    let bare: ProjectionRow = serde_json::from_value(serde_json::json!({
        "player_id": "LAR",
        "stats": {"sack": 45.0, "int": 15.0, "adp_ppr": 90.0},
    }))
    .expect("bare row");
    let mut meta = HashMap::new();
    let rams: PlayerMeta = serde_json::from_value(serde_json::json!({
        "full_name": "Los Angeles Rams", "position": "DEF", "team": "LAR",
    }))
    .expect("player meta");
    meta.insert("LAR".to_string(), rams);
    let league = league(STANDARD, Some(0.5));
    let rules = RosterRules::new(&league.roster_positions);
    let mut warnings = Vec::new();
    let built = build_board(
        &league,
        &draft(),
        &meta,
        &[bare],
        &[],
        &rules,
        &mut warnings,
    );
    assert_eq!(built.players[0].name, "Los Angeles Rams");
}

/// A weekly row: one per player per week, with an opponent except on the bye.
fn weekly(player_id: &str, team: &str, week: u32, opponent: Option<&str>) -> ProjectionRow {
    serde_json::from_value(serde_json::json!({
        "player_id": player_id,
        "stats": {"rec_yd": 60.0},
        "player": {"full_name": "W", "position": "WR", "team": team},
        "week": week,
        "opponent": opponent,
    }))
    .expect("weekly row")
}

#[test]
fn a_week_that_failed_to_download_is_not_everybodys_bye() {
    // Two teams, byes in weeks 5 and 9. Week 12 fetched empty for the whole
    // league — no team has a row for it. Counting that as the emptiest week
    // used to hand week 12 to every player on the board.
    let mut weekly_rows = Vec::new();
    for week in 1..=11 {
        if week != 5 {
            weekly_rows.push(weekly("wr-1", "AAA", week, Some("ZZZ")));
        }
        if week != 9 {
            weekly_rows.push(weekly("wr-2", "BBB", week, Some("ZZZ")));
        }
    }
    let rows = [
        row(
            "wr-1",
            "WR",
            serde_json::json!({"rec_yd": 1200.0, "rec_td": 8.0, "adp_ppr": 12.0}),
        ),
        {
            let mut r = row(
                "wr-2",
                "WR",
                serde_json::json!({"rec_yd": 1100.0, "rec_td": 7.0, "adp_ppr": 20.0}),
            );
            if let Some(meta) = r.player.as_mut() {
                meta.team = Some("BBB".into());
            }
            r
        },
    ];
    let built = build(&league(STANDARD, Some(0.5)), &rows, &weekly_rows);
    let byes: HashMap<&str, Option<u32>> = built
        .players
        .iter()
        .map(|p| (p.player_id.as_str(), p.bye_week))
        .collect();
    assert_eq!(byes["wr-1"], Some(5));
    assert_eq!(byes["wr-2"], Some(9));
}
