//! The translation table, slot by slot and stat id by stat id.

use super::*;
use crate::yahoo_types::{StatCategory, YahooManager, YahooPlayer};

fn slot(position: &str, count: u32) -> RosterSlot {
    RosterSlot {
        position: position.into(),
        count,
    }
}

fn modifier(stat_id: u32, value: f64) -> StatModifier {
    StatModifier { stat_id, value }
}

fn sample_league() -> YahooLeague {
    YahooLeague {
        league_key: "449.l.12345".into(),
        league_id: "12345".into(),
        name: "Wire Wednesday".into(),
        season: "2026".into(),
        num_teams: 12,
        draft_status: "predraft".into(),
        draft_time: Some(1_789_000_000),
        draft_type: Some("live".into()),
        is_auction_draft: false,
        scoring_type: Some("head".into()),
        roster_positions: vec![
            slot("QB", 1),
            slot("WR", 2),
            slot("RB", 2),
            slot("TE", 1),
            slot("W/R/T", 1),
            slot("K", 1),
            slot("DEF", 1),
            slot("BN", 6),
        ],
        stat_modifiers: vec![modifier(4, 0.04), modifier(5, 4.0), modifier(11, 0.5)],
        stat_categories: vec![StatCategory {
            stat_id: 4,
            name: "Passing Yards".into(),
            display: "Pass Yds".into(),
        }],
        ..YahooLeague::default()
    }
}

fn team(key: &str, position: Option<u32>) -> YahooTeam {
    YahooTeam {
        team_key: key.into(),
        team_id: key.rsplit('.').next().unwrap_or("0").into(),
        name: format!("Team {key}"),
        managers: vec![YahooManager::default()],
        draft_position: position,
    }
}

fn result(pick: u32, round: u32, team_key: &str, player_key: &str) -> YahooDraftPick {
    YahooDraftPick {
        pick,
        round,
        team_key: team_key.into(),
        player_key: player_key.into(),
        cost: None,
        is_keeper: None,
    }
}

#[test]
fn every_flex_yahoo_writes_gets_the_apps_name() {
    assert_eq!(roster_position("W/R/T"), "FLEX");
    assert_eq!(roster_position("Q/W/R/T"), "SUPER_FLEX");
    assert_eq!(roster_position("W/R"), "WRRB_FLEX");
    assert_eq!(roster_position("W/T"), "REC_FLEX");
}

#[test]
fn the_plain_slots_pass_through() {
    for position in ["QB", "RB", "WR", "TE", "K", "DEF", "BN", "IR"] {
        assert_eq!(roster_position(position), position);
    }
}

#[test]
fn a_defence_written_any_of_yahoos_ways_is_def() {
    for written in ["DEF", "D", "DST", "D/ST"] {
        assert_eq!(roster_position(written), "DEF");
    }
}

#[test]
fn an_unknown_slot_keeps_its_own_name_rather_than_disappearing() {
    // A seat the app does not know is still a seat: dropping it would shrink
    // the roster and change every replacement level on the board.
    assert_eq!(roster_position("Q/W/R/T/K"), "Q/W/R/T/K");
}

#[test]
fn counts_become_one_entry_per_seat() {
    let expanded = roster_positions(&[slot("QB", 1), slot("WR", 2), slot("BN", 3)]);
    assert_eq!(expanded, ["QB", "WR", "WR", "BN", "BN", "BN"]);
}

#[test]
fn a_zero_count_slot_takes_no_seat() {
    assert!(roster_positions(&[slot("IR", 0)]).is_empty());
}

#[test]
fn the_draft_status_becomes_the_apps_status() {
    assert_eq!(league_status("predraft"), "pre_draft");
    assert_eq!(league_status("draft"), "drafting");
    assert_eq!(league_status("postdraft"), "in_season");
}

#[test]
fn an_unknown_draft_status_is_read_as_not_drafting() {
    assert_eq!(league_status("paused"), "pre_draft");
    assert_eq!(league_status(""), "pre_draft");
}

#[test]
fn the_stat_table_covers_the_documented_ids_and_nothing_twice() {
    let mut seen = std::collections::HashSet::new();
    for (id, keys) in YAHOO_STAT_IDS {
        assert!(seen.insert(*id), "stat id {id} is in the table twice");
        assert!(!keys.is_empty(), "stat id {id} maps to nothing");
    }
    for id in [
        4, 5, 6, 9, 10, 11, 12, 13, 15, 16, 18, 29, 32, 33, 34, 35, 36, 37,
    ] {
        assert!(seen.contains(&id), "stat id {id} is missing");
    }
    // Field goals by distance, then the points-allowed buckets.
    for id in 19..=23 {
        assert!(seen.contains(&id), "field goal id {id} is missing");
    }
    for id in 50..=56 {
        assert!(seen.contains(&id), "points-allowed id {id} is missing");
    }
}

#[test]
fn every_key_in_the_table_is_one_the_scoring_engine_could_use() {
    // The scoring engine dots the league's settings against Sleeper's stat
    // keys, so a typo here would silently score zero.
    for (_, keys) in YAHOO_STAT_IDS {
        for key in *keys {
            assert!(
                key.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "{key} is not a Sleeper-shaped stat key"
            );
        }
    }
}

#[test]
fn the_common_offensive_rules_map_by_id() {
    let scoring = scoring_settings(&[
        modifier(4, 0.04),
        modifier(5, 4.0),
        modifier(6, -1.0),
        modifier(9, 0.1),
        modifier(10, 6.0),
        modifier(11, 0.5),
        modifier(12, 0.1),
        modifier(13, 6.0),
        modifier(18, -2.0),
    ]);
    assert_eq!(scoring.get("pass_yd"), Some(&0.04));
    assert_eq!(scoring.get("pass_td"), Some(&4.0));
    assert_eq!(scoring.get("pass_int"), Some(&-1.0));
    assert_eq!(scoring.get("rush_yd"), Some(&0.1));
    assert_eq!(scoring.get("rush_td"), Some(&6.0));
    assert_eq!(scoring.get("rec"), Some(&0.5));
    assert_eq!(scoring.get("rec_yd"), Some(&0.1));
    assert_eq!(scoring.get("rec_td"), Some(&6.0));
    assert_eq!(scoring.get("fum_lost"), Some(&-2.0));
}

#[test]
fn the_kicking_and_defence_rules_map_by_id() {
    let scoring = scoring_settings(&[
        modifier(19, 3.0),
        modifier(23, 5.0),
        modifier(29, 1.0),
        modifier(32, 1.0),
        modifier(33, 2.0),
        modifier(34, 2.0),
        modifier(35, 6.0),
        modifier(36, 2.0),
        modifier(37, 2.0),
        modifier(50, 10.0),
        modifier(56, -4.0),
    ]);
    assert_eq!(scoring.get("fgm_0_19"), Some(&3.0));
    assert_eq!(scoring.get("fgm_50p"), Some(&5.0));
    assert_eq!(scoring.get("xpm"), Some(&1.0));
    assert_eq!(scoring.get("sack"), Some(&1.0));
    assert_eq!(scoring.get("int"), Some(&2.0));
    assert_eq!(scoring.get("fum_rec"), Some(&2.0));
    assert_eq!(scoring.get("def_td"), Some(&6.0));
    assert_eq!(scoring.get("safe"), Some(&2.0));
    assert_eq!(scoring.get("blk_kick"), Some(&2.0));
    assert_eq!(scoring.get("pts_allow_0"), Some(&10.0));
    assert_eq!(scoring.get("pts_allow_35p"), Some(&-4.0));
}

#[test]
fn one_two_point_rule_pays_out_however_the_conversion_was_scored() {
    let scoring = scoring_settings(&[modifier(16, 2.0)]);
    assert_eq!(scoring.get("pass_2pt"), Some(&2.0));
    assert_eq!(scoring.get("rush_2pt"), Some(&2.0));
    assert_eq!(scoring.get("rec_2pt"), Some(&2.0));
}

#[test]
fn an_id_with_no_sleeper_equivalent_is_dropped_rather_than_guessed() {
    let scoring = scoring_settings(&[modifier(9_999, 1.0), modifier(4, 0.04)]);
    assert_eq!(scoring.len(), 1);
    assert_eq!(scoring.get("pass_yd"), Some(&0.04));
}

#[test]
fn a_league_maps_onto_the_apps_shape() {
    let mapped = league(&sample_league());
    assert_eq!(mapped.league_id, "449.l.12345");
    assert_eq!(mapped.name, "Wire Wednesday");
    assert_eq!(mapped.season, "2026");
    assert_eq!(mapped.status, "pre_draft");
    assert_eq!(mapped.total_rosters, 12);
    assert_eq!(
        mapped.roster_positions,
        [
            "QB", "WR", "WR", "RB", "RB", "TE", "FLEX", "K", "DEF", "BN", "BN", "BN", "BN", "BN",
            "BN"
        ]
    );
    assert_eq!(mapped.scoring_settings.get("pass_yd"), Some(&0.04));
    // Documented defaults: Yahoo has no separate draft resource and this lane
    // does not read last season or the playoff knobs.
    assert!(mapped.draft_id.is_none());
    assert!(mapped.previous_league_id.is_none());
    assert!(mapped.settings.playoff_week_start.is_none());
}

#[test]
fn a_yahoo_player_id_is_prefixed_and_stripped_of_the_game_key() {
    assert_eq!(player_id("449.p.30977"), "yahoo:30977");
    assert_eq!(player_id("30977"), "yahoo:30977");
}

#[test]
fn picks_take_their_slot_from_the_teams_draft_position() {
    let teams = [team("449.l.1.t.1", Some(4)), team("449.l.1.t.2", Some(9))];
    let mapped = picks(
        &[
            result(1, 1, "449.l.1.t.2", "449.p.100"),
            result(2, 1, "449.l.1.t.1", "449.p.200"),
        ],
        &teams,
        &HashMap::new(),
    );
    assert_eq!(mapped.len(), 2);
    assert_eq!(mapped[0].pick_no, 1);
    assert_eq!(mapped[0].round, 1);
    assert_eq!(mapped[0].draft_slot, 9);
    assert_eq!(mapped[0].player_id, "yahoo:100");
    assert_eq!(mapped[0].picked_by.as_deref(), Some("449.l.1.t.2"));
    assert_eq!(mapped[1].draft_slot, 4);
}

#[test]
fn a_team_with_no_draft_position_yet_is_slot_zero_rather_than_slot_one() {
    let mapped = picks(
        &[result(1, 1, "449.l.1.t.3", "449.p.100")],
        &[team("449.l.1.t.3", None)],
        &HashMap::new(),
    );
    assert_eq!(mapped[0].draft_slot, 0);
}

#[test]
fn a_recorded_pick_with_no_player_is_not_a_pick_yet() {
    let mapped = picks(
        &[
            result(1, 1, "449.l.1.t.1", "449.p.100"),
            result(2, 1, "449.l.1.t.2", ""),
        ],
        &[team("449.l.1.t.1", Some(1))],
        &HashMap::new(),
    );
    assert_eq!(mapped.len(), 1);
}

#[test]
fn a_known_player_fills_in_the_picks_label() {
    let mut pool = HashMap::new();
    pool.insert("449.p.100".to_string(), sample_player());
    let mapped = picks(
        &[result(1, 1, "449.l.1.t.1", "449.p.100")],
        &[team("449.l.1.t.1", Some(1))],
        &pool,
    );
    let meta = mapped[0].metadata.as_ref().expect("the pool knew this one");
    assert_eq!(meta.first_name.as_deref(), Some("Ja'Marr"));
    assert_eq!(meta.last_name.as_deref(), Some("Chase"));
    assert_eq!(meta.position.as_deref(), Some("WR"));
    assert_eq!(meta.team.as_deref(), Some("CIN"));
}

fn sample_player() -> YahooPlayer {
    YahooPlayer {
        player_key: "449.p.100".into(),
        player_id: "100".into(),
        full_name: "Ja'Marr Chase".into(),
        first: "Ja'Marr".into(),
        last: "Chase".into(),
        editorial_team_abbr: "Cin".into(),
        display_position: "WR".into(),
        eligible_positions: vec!["WR".into(), "W/R/T".into()],
        status: Some("Q".into()),
        bye_week: Some(10),
        uniform_number: Some("1".into()),
        is_keeper: None,
    }
}

#[test]
fn a_player_maps_onto_the_apps_row() {
    let mapped = player(&sample_player());
    assert_eq!(mapped.id, "yahoo:100");
    assert_eq!(mapped.player_key, "449.p.100");
    assert_eq!(mapped.meta.full_name.as_deref(), Some("Ja'Marr Chase"));
    assert_eq!(mapped.meta.position.as_deref(), Some("WR"));
    // Yahoo writes "Cin"; every other source in the app writes "CIN".
    assert_eq!(mapped.meta.team.as_deref(), Some("CIN"));
    assert_eq!(mapped.meta.injury_status.as_deref(), Some("Q"));
    assert_eq!(mapped.bye_week, Some(10));
    // No Yahoo source: documented defaults.
    assert!(mapped.meta.years_exp.is_none());
    assert!(mapped.meta.age.is_none());
}

#[test]
fn the_eligible_positions_drop_the_flex_seats_a_player_merely_fits() {
    let mapped = player(&sample_player());
    assert_eq!(
        mapped.meta.fantasy_positions.as_deref(),
        Some(["WR".to_string()].as_slice())
    );
}

#[test]
fn a_defence_maps_to_def_on_both_sides() {
    let mapped = player(&YahooPlayer {
        display_position: "DEF".into(),
        eligible_positions: vec!["DEF".into()],
        ..sample_player()
    });
    assert_eq!(mapped.meta.position.as_deref(), Some("DEF"));
    assert_eq!(
        mapped.meta.fantasy_positions.as_deref(),
        Some(["DEF".to_string()].as_slice())
    );
}

#[test]
fn the_crosswalk_key_is_normalised_the_same_way_the_csv_import_normalises() {
    let key = player(&sample_player()).crosswalk_key();
    assert_eq!(
        key,
        (
            crate::second_opinion::normalize_name("Ja'Marr Chase"),
            "CIN".to_string(),
            "WR".to_string()
        )
    );
}

#[test]
fn a_page_of_players_maps_row_for_row() {
    let rows = [
        sample_player(),
        YahooPlayer {
            player_key: "449.p.200".into(),
            ..sample_player()
        },
    ];
    let mapped = players(&rows);
    assert_eq!(mapped.len(), 2);
    assert_eq!(mapped[1].id, "yahoo:200");
}

#[test]
fn the_stat_ids_yahoo_leagues_actually_use_all_land_on_a_scoring_key() {
    // The ids added after the first pass: the negatives a kicker and a
    // ball-carrier are docked for, which a league that scores them would
    // otherwise have quietly counted as zero.
    let scoring = scoring_settings(&[
        modifier(17, -1.0),
        modifier(25, -3.0),
        modifier(26, -2.0),
        modifier(27, -2.0),
        modifier(28, -1.0),
        modifier(30, -1.0),
    ]);
    assert_eq!(scoring.get("fum"), Some(&-1.0));
    assert_eq!(scoring.get("fgmiss_0_19"), Some(&-3.0));
    assert_eq!(scoring.get("fgmiss_20_29"), Some(&-2.0));
    assert_eq!(scoring.get("fgmiss_30_39"), Some(&-2.0));
    assert_eq!(scoring.get("fgmiss_40_49"), Some(&-1.0));
    assert_eq!(scoring.get("xpmiss"), Some(&-1.0));
    // Fumbles and fumbles lost are two rules, not one.
    let both = scoring_settings(&[modifier(17, -1.0), modifier(18, -2.0)]);
    assert_eq!(both.get("fum"), Some(&-1.0));
    assert_eq!(both.get("fum_lost"), Some(&-2.0));
}

#[test]
fn no_two_stat_ids_claim_the_same_scoring_key() {
    let mut seen: HashMap<&str, u32> = HashMap::new();
    for (id, keys) in YAHOO_STAT_IDS {
        for key in *keys {
            if let Some(other) = seen.insert(key, *id) {
                panic!("stat ids {other} and {id} both write {key}");
            }
        }
    }
}

#[test]
fn a_scored_category_the_app_cannot_read_is_named_rather_than_dropped() {
    let mut league = sample_league();
    // Yahoo id 14 is return yardage, which the app has no key for.
    league.stat_modifiers.push(modifier(14, 0.04));
    league.stat_categories.push(StatCategory {
        stat_id: 14,
        name: "Return Yards".into(),
        display: "Ret Yds".into(),
    });
    assert_eq!(unscored_stats(&league), ["Return Yards (14)"]);
    let warning = unscored_stats_warning(&league).expect("a warning");
    assert!(warning.contains("Return Yards (14)"), "{warning}");
    assert!(warning.contains("a category"), "{warning}");
    // The ones it can read are not mentioned.
    assert!(!warning.contains("Passing Yards"), "{warning}");
}

#[test]
fn several_unreadable_categories_are_listed_together_and_read_as_plural() {
    let mut league = sample_league();
    league.stat_modifiers.push(modifier(14, 0.04));
    league.stat_modifiers.push(modifier(78, 1.0));
    let warning = unscored_stats_warning(&league).expect("a warning");
    // No `stat_categories` row for either, so the id is what gets reported.
    assert!(warning.contains("Yahoo stat 14"), "{warning}");
    assert!(warning.contains("Yahoo stat 78"), "{warning}");
    assert!(warning.contains("categories"), "{warning}");
    assert!(warning.contains("they are"), "{warning}");
}

#[test]
fn a_league_whose_every_rule_is_understood_gets_no_warning() {
    assert!(unscored_stats_warning(&sample_league()).is_none());
    assert!(unscored_stats(&sample_league()).is_empty());
}

#[test]
fn a_category_yahoo_lists_but_pays_nothing_for_is_not_worth_a_warning() {
    let mut league = sample_league();
    league.stat_modifiers.push(modifier(14, 0.0));
    assert!(
        unscored_stats_warning(&league).is_none(),
        "a rule worth zero costs the board nothing"
    );
}
