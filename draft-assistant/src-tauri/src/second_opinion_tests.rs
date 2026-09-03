//! Tests for the imported second opinion: the parser, the name normaliser,
//! the matcher, and the rec-card reason.
//!
//! The good-file cases run against a trimmed copy of what the real projections
//! script wrote on 2026-08-28 (`tests/fixtures/second_opinion_sample.csv`) —
//! forty rows of the actual file, chosen to include the suffixes, the
//! apostrophes and the two team defences that the hand-written cases below
//! then pin one at a time.

use super::*;

fn sample() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("second_opinion_sample.csv");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn board_player(name: &str, position: &str, team: Option<&str>, rank: u32) -> BoardPlayer {
    BoardPlayer {
        player_id: name.to_string(),
        name: name.to_string(),
        position: position.to_string(),
        team: team.map(str::to_string),
        bye_week: None,
        points: 100.0,
        bonus_points: 0.0,
        vorp: 10.0,
        tier: 1,
        position_rank: rank,
        overall_rank: rank,
        adp: None,
        injury_status: None,
        sleeper_pts_ppr: None,
        second_opinion: None,
        weekly_cv: None,
    }
}

// ---------- the parser ----------

#[test]
fn the_real_export_parses_and_names_its_source() {
    let table = parse(&sample(), 1_700_000_000).expect("sample parses");
    assert_eq!(table.len(), 40, "the trimmed sample has forty rows");
    assert_eq!(table.source, "Clay");
    assert_eq!(table.loaded_at, 1_700_000_000);
}

#[test]
fn positional_rank_is_computed_within_each_position() {
    let table = parse(&sample(), 0).unwrap();
    // Jahmyr Gibbs is overall rank 1 in the file and the first RB in it.
    let gibbs = table.find("Jahmyr Gibbs", "RB", Some("DET")).unwrap();
    assert_eq!(table.opinion(gibbs).positional_rank, 1);
    assert_eq!(table.opinion(gibbs).overall_rank, 1);
    // Ja'Marr Chase is overall 3 but the first WR, so WR1.
    let chase = table.find("Ja'Marr Chase", "WR", Some("CIN")).unwrap();
    assert_eq!(table.opinion(chase).positional_rank, 1);
    assert_eq!(table.opinion(chase).overall_rank, 3);
    // Every position starts at 1 and runs without a gap.
    for position in ["QB", "RB", "WR", "TE", "K", "DEF"] {
        let mut ranks: Vec<u32> = table
            .rows
            .iter()
            .filter(|r| r.position == position)
            .map(|r| r.positional_rank)
            .collect();
        ranks.sort_unstable();
        let want: Vec<u32> = (1..=ranks.len() as u32).collect();
        assert_eq!(ranks, want, "{position} ranks are not 1..n");
    }
}

#[test]
fn a_name_quoted_because_it_holds_a_comma_survives_the_parse() {
    let text = "rank,name,position,team,projected_fantasy_points,source\n\
                1,\"Robinson, Jr., Brian\",RB,ATL,200.0,clayprojections\n\
                2,Ja'Marr Chase,WR,CIN,190.0,clayprojections\n";
    let table = parse(text, 0).expect("quoted names parse");
    assert_eq!(table.len(), 2);
    assert!(table
        .find("Robinson, Jr., Brian", "RB", Some("ATL"))
        .is_some());
}

#[test]
fn a_file_missing_a_column_is_refused_in_plain_words() {
    let text = "rank,player,position,projected_fantasy_points\n1,Someone,RB,200.0\n";
    let error = parse(text, 0).expect_err("a file with no name column is not importable");
    assert!(error.contains("\"name\" column"), "unhelpful: {error}");
    assert!(!error.contains("Err("), "raw error leaked: {error}");
}

#[test]
fn a_header_less_file_is_refused_rather_than_read_as_data() {
    // The first line becomes the header, and it has none of the columns.
    let text = "1,Jahmyr Gibbs,RB,DET,331.0\n2,Bijan Robinson,RB,ATL,315.0\n";
    let error = parse(text, 0).expect_err("a header-less file is not importable");
    assert!(error.contains("column"), "unhelpful: {error}");
}

#[test]
fn an_empty_file_and_a_headers_only_file_are_both_refused() {
    assert!(parse("", 0).is_err());
    let error = parse(
        "rank,name,position,team,projected_fantasy_points,source\n",
        0,
    )
    .expect_err("headers with no rows import nothing");
    assert!(error.contains("no player rows"), "unhelpful: {error}");
}

#[test]
fn rows_that_cannot_be_read_are_skipped_not_fatal() {
    let text = "rank,name,position,team,projected_fantasy_points,source\n\
                1,Good Player,RB,ATL,200.0,clayprojections\n\
                ,No Rank,RB,ATL,190.0,clayprojections\n\
                3,,RB,ATL,180.0,clayprojections\n";
    let table = parse(text, 0).expect("the one good row is enough");
    assert_eq!(table.len(), 1);
}

// ---------- the normaliser ----------

#[test]
fn suffixes_punctuation_and_case_all_normalise_away() {
    for (a, b) in [
        ("Travis Etienne Jr.", "Travis Etienne"),
        ("Kenneth Walker III", "Kenneth Walker"),
        ("Marvin Harrison Jr", "marvin harrison"),
        ("Ja'Marr Chase", "JaMarr Chase"),
        ("De'Von Achane", "Devon Achane"),
        ("D.J. Moore", "DJ Moore"),
        ("Amon-Ra St. Brown", "Amon Ra St. Brown"),
        ("Michael  Pittman   Jr.", "Michael Pittman"),
        ("Denver Broncos D/ST", "Denver Broncos"),
    ] {
        assert_eq!(
            normalize_name(a),
            normalize_name(b),
            "{a:?} and {b:?} should normalise the same"
        );
    }
}

#[test]
fn different_players_do_not_collapse_onto_each_other() {
    assert_ne!(normalize_name("Josh Allen"), normalize_name("Keenan Allen"));
    assert_ne!(
        normalize_name("Michael Thomas"),
        normalize_name("Michael Pittman")
    );
}

#[test]
fn a_name_that_is_nothing_but_a_suffix_keeps_itself() {
    // Otherwise every such row would normalise to "" and match everyone.
    assert_eq!(normalize_name("III"), "iii");
    assert!(!normalize_name("Jr.").is_empty());
}

#[test]
fn a_run_together_name_still_finds_its_row() {
    // "St.Brown" loses its space to the punctuation strip, so the spaced key
    // misses and the spaceless fallback is what catches it.
    let text = "rank,name,position,team,projected_fantasy_points,source\n\
                1,Amon-Ra St. Brown,WR,DET,250.0,clayprojections\n";
    let table = parse(text, 0).unwrap();
    assert!(table.find("Amon-Ra St.Brown", "WR", Some("DET")).is_some());
}

#[test]
fn a_defence_position_is_read_under_every_spelling() {
    for spelling in ["DST", "D/ST", "dst", "Defense", "DEF"] {
        assert_eq!(normalize_position(spelling), "DEF");
    }
    assert_eq!(normalize_position("wr"), "WR");
}

// ---------- the matcher ----------

#[test]
fn matching_reports_the_hits_and_the_misses() {
    let table = parse(&sample(), 0).unwrap();
    let mut board = vec![
        board_player("Jahmyr Gibbs", "RB", Some("DET"), 4),
        board_player("Travis Etienne", "RB", Some("NO"), 30),
        board_player("Nobody At All", "WR", Some("SEA"), 40),
    ];
    let report = apply(&table, &mut board);
    assert_eq!(report.total, 40);
    assert_eq!(report.matched, 2, "two of the forty rows found a player");
    assert_eq!(
        report.message(),
        "Second opinion loaded: 2 of 40 players matched"
    );
    assert_eq!(board[0].second_opinion.as_ref().unwrap().positional_rank, 1);
    assert_eq!(board[1].second_opinion.as_ref().unwrap().source, "Clay");
    assert!(board[2].second_opinion.is_none());
}

#[test]
fn a_player_who_changed_team_still_matches() {
    // Kenneth Walker III is on KC in the file; the board says SEA.
    let table = parse(&sample(), 0).unwrap();
    let mut board = vec![board_player("Kenneth Walker", "RB", Some("SEA"), 12)];
    assert_eq!(apply(&table, &mut board).matched, 1);
    assert!(board[0].second_opinion.is_some());
}

#[test]
fn the_position_has_to_agree_even_when_the_name_does() {
    let table = parse(&sample(), 0).unwrap();
    let mut board = vec![board_player("Jahmyr Gibbs", "WR", Some("DET"), 1)];
    assert_eq!(apply(&table, &mut board).matched, 0);
    assert!(board[0].second_opinion.is_none());
}

#[test]
fn a_team_defence_matches_the_sleeper_spelling() {
    let table = parse(&sample(), 0).unwrap();
    let mut board = vec![board_player("Denver Broncos", "DEF", Some("DEN"), 5)];
    assert_eq!(apply(&table, &mut board).matched, 1);
}

#[test]
fn re_applying_a_table_clears_players_it_no_longer_knows() {
    let table = parse(&sample(), 0).unwrap();
    let mut board = vec![board_player("Nobody At All", "WR", Some("SEA"), 40)];
    board[0].second_opinion = Some(SecondOpinion {
        positional_rank: 3,
        overall_rank: 9,
        source: "Stale".into(),
    });
    apply(&table, &mut board);
    assert!(
        board[0].second_opinion.is_none(),
        "a stale opinion lingered"
    );
}

// ---------- the rec-card reason ----------

fn with_opinion(board_rank: u32, csv_rank: u32, adp: Option<f64>) -> BoardPlayer {
    let mut player = board_player("Someone", "WR", Some("SEA"), board_rank);
    player.adp = adp;
    player.second_opinion = Some(SecondOpinion {
        positional_rank: csv_rank,
        overall_rank: 22,
        source: "Clay".into(),
    });
    player
}

#[test]
fn the_adjustment_runs_both_ways_and_scales_with_the_gap() {
    // Board WR21, Clay WR9: twelve spots the user's way, so a bump up.
    let (delta, reason) = rec_adjustment(&with_opinion(21, 9, None), 12).expect("an adjustment");
    assert!(reason.starts_with("Clay has him WR9"), "{reason}");
    assert!(reason.contains("this board has him WR21"), "{reason}");
    assert!((delta - 3.0).abs() < 1e-9, "{delta}");

    // The other direction is a warning, not silence: Clay has him thirty-odd
    // places behind where this board does.
    let (delta, reason) = rec_adjustment(&with_opinion(9, 41, None), 12).expect("an adjustment");
    assert!((delta + 8.0).abs() < 1e-9, "capped at -8: {delta}");
    assert!(reason.contains("behind this board's WR9"), "{reason}");

    // Bigger disagreement, bigger adjustment — up to the cap.
    let small = rec_adjustment(&with_opinion(21, 9, None), 12).expect("a").0;
    let large = rec_adjustment(&with_opinion(45, 9, None), 12).expect("a").0;
    assert!(large > small, "{large} vs {small}");
    assert!(large <= 8.0, "capped at +8: {large}");

    // A small disagreement is noise, whichever way it points.
    assert!(rec_adjustment(&with_opinion(14, 9, None), 12).is_none());
    assert!(rec_adjustment(&with_opinion(9, 14, None), 12).is_none());
    // A player with nothing imported says nothing.
    assert!(rec_adjustment(&board_player("Someone", "WR", None, 21), 12).is_none());
}

#[test]
fn the_reason_counts_the_market_lag_in_rounds_when_there_is_an_adp() {
    // Clay's overall rank 22, ADP 58, twelve-team league: three rounds late.
    let (_, reason) = rec_adjustment(&with_opinion(21, 9, Some(58.0)), 12).expect("a reason");
    assert_eq!(reason, "Clay has him WR9 — market is 3 rounds late");
    // One round reads as one round, not "1 rounds".
    let (_, reason) = rec_adjustment(&with_opinion(21, 9, Some(36.0)), 12).expect("a reason");
    assert_eq!(reason, "Clay has him WR9 — market is 1 round late");
    // An ADP that is ahead of the source falls back to the plain comparison.
    let (_, reason) = rec_adjustment(&with_opinion(21, 9, Some(10.0)), 12).expect("a reason");
    assert!(reason.contains("this board has him WR21"), "{reason}");
    // The market-lag line belongs to the direction that earns it: a source
    // that is *down* on the player never reads as the market being late.
    let (_, reason) = rec_adjustment(&with_opinion(9, 41, Some(58.0)), 12).expect("a reason");
    assert!(!reason.contains("late"), "{reason}");
}

// ---------- the stored copy ----------

#[test]
fn a_missing_stored_file_is_not_an_error() {
    let dir = std::env::temp_dir().join(format!("second-opinion-none-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    assert!(load(&dir).unwrap().is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_stored_file_that_stopped_parsing_is_reported_not_swallowed() {
    let dir = std::env::temp_dir().join(format!("second-opinion-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(stored_path(&dir), "not,a,projections,file\n1,2,3,4\n").unwrap();
    assert!(load(&dir).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_stored_copy_of_the_real_export_round_trips() {
    let dir = std::env::temp_dir().join(format!("second-opinion-ok-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(stored_path(&dir), sample()).unwrap();
    let table = load(&dir).unwrap().expect("a stored table");
    assert_eq!(table.len(), 40);
    assert_eq!(table.source, "Clay");
    std::fs::remove_dir_all(&dir).ok();
}
