use super::build;
use crate::sleeper::PlayerMeta;
use crate::yahoo_map::player;
use crate::yahoo_types::YahooPlayer;
use std::collections::HashMap;

fn sleeper_row(full: &str, position: &str, team: &str) -> PlayerMeta {
    let mut parts = full.split_whitespace();
    PlayerMeta {
        full_name: Some(full.to_string()),
        first_name: parts.next().map(str::to_string),
        last_name: parts.next().map(str::to_string),
        position: Some(position.to_string()),
        team: Some(team.to_string()),
        fantasy_positions: Some(vec![position.to_string()]),
        injury_status: None,
        years_exp: None,
        age: None,
    }
}

/// Sleeper's defences: no `full_name`, the city and the mascot split across
/// the two name fields, and the team abbreviation as the id.
fn sleeper_defence(city: &str, mascot: &str, team: &str) -> PlayerMeta {
    PlayerMeta {
        full_name: None,
        first_name: Some(city.to_string()),
        last_name: Some(mascot.to_string()),
        position: Some("DEF".to_string()),
        team: Some(team.to_string()),
        fantasy_positions: Some(vec!["DEF".to_string()]),
        injury_status: None,
        years_exp: None,
        age: None,
    }
}

fn yahoo(
    key: &str,
    full: &str,
    first: &str,
    last: &str,
    team: &str,
    position: &str,
) -> YahooPlayer {
    YahooPlayer {
        player_key: key.to_string(),
        player_id: key.rsplit('.').next().unwrap_or(key).to_string(),
        full_name: full.to_string(),
        first: first.to_string(),
        last: last.to_string(),
        editorial_team_abbr: team.to_string(),
        display_position: position.to_string(),
        eligible_positions: vec![position.to_string()],
        status: None,
        bye_week: None,
        uniform_number: None,
        is_keeper: None,
    }
}

fn dictionary(rows: &[(&str, PlayerMeta)]) -> HashMap<String, PlayerMeta> {
    rows.iter()
        .map(|(id, meta)| ((*id).to_string(), meta.clone()))
        .collect()
}

#[test]
fn a_plain_name_team_and_position_match_carries_the_sleeper_id() {
    let sleeper = dictionary(&[("4034", sleeper_row("Christian McCaffrey", "RB", "SF"))]);
    let pool = vec![player(&yahoo(
        "449.p.29399",
        "Christian McCaffrey",
        "Christian",
        "McCaffrey",
        "SF",
        "RB",
    ))];
    let crosswalk = build(&pool, &sleeper);
    assert_eq!(crosswalk.id_for("449.p.29399"), Some("4034"));
    assert_eq!(crosswalk.unmatched, 0);
    assert!(crosswalk.warning().is_none());
    // The row the board gets is Sleeper's, so years_exp and the rest survive.
    assert!(crosswalk.player_meta.contains_key("4034"));
}

#[test]
fn an_apostrophe_and_a_suffix_are_spelled_the_same_way_on_both_sides() {
    let sleeper = dictionary(&[
        ("6794", sleeper_row("Ja'Marr Chase", "WR", "CIN")),
        ("7564", sleeper_row("Michael Pittman Jr.", "WR", "IND")),
        ("4881", sleeper_row("D.J. Moore", "WR", "CHI")),
    ]);
    let pool = vec![
        // Yahoo writes the team mixed-case; the mapper upper-cases it.
        player(&yahoo(
            "449.p.30977",
            "Ja'Marr Chase",
            "Ja'Marr",
            "Chase",
            "Cin",
            "WR",
        )),
        player(&yahoo(
            "449.p.32692",
            "Michael Pittman",
            "Michael",
            "Pittman",
            "Ind",
            "WR",
        )),
        player(&yahoo(
            "449.p.31002",
            "DJ Moore",
            "DJ",
            "Moore",
            "Chi",
            "WR",
        )),
    ];
    let crosswalk = build(&pool, &sleeper);
    assert_eq!(crosswalk.id_for("449.p.30977"), Some("6794"));
    assert_eq!(crosswalk.id_for("449.p.32692"), Some("7564"));
    assert_eq!(crosswalk.id_for("449.p.31002"), Some("4881"));
    assert_eq!(crosswalk.unmatched, 0);
}

#[test]
fn a_defence_is_matched_by_its_team_because_its_name_never_lines_up() {
    let sleeper = dictionary(&[("BAL", sleeper_defence("Baltimore", "Ravens", "BAL"))]);
    let pool = vec![player(&yahoo(
        "449.p.100001",
        "Baltimore",
        "Baltimore",
        "",
        "Bal",
        "DEF",
    ))];
    let crosswalk = build(&pool, &sleeper);
    assert_eq!(crosswalk.id_for("449.p.100001"), Some("BAL"));
    assert_eq!(crosswalk.unmatched, 0);
}

#[test]
fn a_player_who_changed_teams_still_matches_when_the_name_is_unambiguous() {
    let sleeper = dictionary(&[("1234", sleeper_row("Saquon Barkley", "RB", "NYG"))]);
    let pool = vec![player(&yahoo(
        "449.p.31056",
        "Saquon Barkley",
        "Saquon",
        "Barkley",
        "Phi",
        "RB",
    ))];
    assert_eq!(build(&pool, &sleeper).id_for("449.p.31056"), Some("1234"));
}

#[test]
fn two_sleeper_players_of_one_name_never_claim_a_traded_yahoo_player() {
    let sleeper = dictionary(&[
        ("a1", sleeper_row("Mike Williams", "WR", "LAC")),
        ("b2", sleeper_row("Mike Williams", "WR", "NYJ")),
    ]);
    // Neither team matches, and the loose key is ambiguous, so nothing is
    // guessed at.
    let pool = vec![player(&yahoo(
        "449.p.28000",
        "Mike Williams",
        "Mike",
        "Williams",
        "Pit",
        "WR",
    ))];
    let crosswalk = build(&pool, &sleeper);
    assert_eq!(crosswalk.id_for("449.p.28000"), Some("yahoo:28000"));
    assert_eq!(crosswalk.unmatched, 1);
    // …but the one whose team does line up is still found exactly.
    let pool = vec![player(&yahoo(
        "449.p.28000",
        "Mike Williams",
        "Mike",
        "Williams",
        "Nyj",
        "WR",
    ))];
    assert_eq!(build(&pool, &sleeper).id_for("449.p.28000"), Some("b2"));
}

#[test]
fn an_unmatched_player_keeps_his_yahoo_row_and_is_counted() {
    let sleeper = dictionary(&[("4034", sleeper_row("Christian McCaffrey", "RB", "SF"))]);
    let pool = vec![player(&yahoo(
        "449.p.99999",
        "Nobody Atall",
        "Nobody",
        "Atall",
        "Was",
        "TE",
    ))];
    let crosswalk = build(&pool, &sleeper);
    assert_eq!(crosswalk.id_for("449.p.99999"), Some("yahoo:99999"));
    assert_eq!(crosswalk.unmatched, 1);
    let meta = crosswalk
        .player_meta
        .get("yahoo:99999")
        .expect("the unmatched player is still on the board");
    assert_eq!(meta.full_name.as_deref(), Some("Nobody Atall"));
    assert_eq!(meta.team.as_deref(), Some("WAS"));
    let warning = crosswalk.warning().expect("one player went unmatched");
    assert!(
        warning.starts_with("1 Yahoo players had no Sleeper match"),
        "{warning}"
    );
}

#[test]
fn a_position_mismatch_is_not_a_match() {
    let sleeper = dictionary(&[("4034", sleeper_row("Christian McCaffrey", "RB", "SF"))]);
    let pool = vec![player(&yahoo(
        "449.p.29399",
        "Christian McCaffrey",
        "Christian",
        "McCaffrey",
        "SF",
        "WR",
    ))];
    assert_eq!(build(&pool, &sleeper).unmatched, 1);
}

#[test]
fn a_dictionary_row_with_no_position_is_skipped_rather_than_indexed() {
    let mut nameless = sleeper_row("Christian McCaffrey", "RB", "SF");
    nameless.position = None;
    let sleeper = dictionary(&[("4034", nameless)]);
    let pool = vec![player(&yahoo(
        "449.p.29399",
        "Christian McCaffrey",
        "Christian",
        "McCaffrey",
        "SF",
        "RB",
    ))];
    assert_eq!(build(&pool, &sleeper).unmatched, 1);
}

#[test]
fn a_defence_matches_across_the_two_spellings_of_one_franchise() {
    // The failure this prevents: Yahoo writes Jacksonville as JAC and Sleeper
    // as JAX, so that league's defence found no Sleeper row, sat on the board
    // with no projection and was counted as an unmatched player. Washington
    // (WSH/WAS) and Las Vegas (LVR/LV) did the same.
    let sleeper = dictionary(&[
        ("JAX", sleeper_defence("Jacksonville", "Jaguars", "JAX")),
        ("WAS", sleeper_defence("Washington", "Commanders", "WAS")),
        ("LV", sleeper_defence("Las Vegas", "Raiders", "LV")),
    ]);
    let pool = vec![
        player(&yahoo(
            "449.p.100010",
            "Jacksonville",
            "Jacksonville",
            "",
            "Jac",
            "DEF",
        )),
        player(&yahoo(
            "449.p.100011",
            "Washington",
            "Washington",
            "",
            "WSH",
            "DEF",
        )),
        player(&yahoo(
            "449.p.100012",
            "Las Vegas",
            "Las",
            "Vegas",
            "LVR",
            "DEF",
        )),
    ];
    let crosswalk = build(&pool, &sleeper);
    assert_eq!(crosswalk.id_for("449.p.100010"), Some("JAX"));
    assert_eq!(crosswalk.id_for("449.p.100011"), Some("WAS"));
    assert_eq!(crosswalk.id_for("449.p.100012"), Some("LV"));
    assert_eq!(crosswalk.unmatched, 0);
}

#[test]
fn a_defence_with_no_team_abbreviation_is_found_by_its_name() {
    // Yahoo leaves `editorial_team_abbr` off the odd defence row, and the
    // abbreviation was the only thing a defence was ever matched on.
    let sleeper = dictionary(&[("BAL", sleeper_defence("Baltimore", "Ravens", "BAL"))]);
    let pool = vec![player(&yahoo(
        "449.p.100013",
        "Baltimore Ravens",
        "Baltimore",
        "Ravens",
        "",
        "DEF",
    ))];
    let crosswalk = build(&pool, &sleeper);
    assert_eq!(crosswalk.id_for("449.p.100013"), Some("BAL"));
    assert_eq!(crosswalk.unmatched, 0);
}

#[test]
fn a_skill_player_whose_team_is_spelled_the_other_way_still_matches_exactly() {
    // The abbreviation folding is not a defence-only fix: an exact match is
    // name, team and position, and JAC never equalled JAX on any of them.
    let sleeper = dictionary(&[("9999", sleeper_row("Travis Etienne", "RB", "JAX"))]);
    let pool = vec![player(&yahoo(
        "449.p.32700",
        "Travis Etienne",
        "Travis",
        "Etienne",
        "Jac",
        "RB",
    ))];
    assert_eq!(build(&pool, &sleeper).id_for("449.p.32700"), Some("9999"));
}
