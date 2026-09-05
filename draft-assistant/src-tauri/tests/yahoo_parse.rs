//! Every Yahoo struct, parsed out of a payload shaped like the real one.
//!
//! The fixtures under `tests/fixtures/yahoo/` keep Yahoo's quirks intact — the
//! numeric string keys, the list of one-key objects, the doubly-wrapped
//! attribute arrays, the numbers sent as strings — because those quirks are
//! the only hard part of reading this API. The wire tests serve these same
//! files off a socket; here they are parsed directly.

use draft_assistant_lib::yahoo_parse as parse;
use serde_json::Value;

fn json(text: &str) -> Value {
    serde_json::from_str(text).expect("the fixture is valid JSON")
}

const USER_LEAGUES: &str = include_str!("fixtures/yahoo/user_leagues.json");
const LEAGUE: &str = include_str!("fixtures/yahoo/league_settings.json");
const TEAMS: &str = include_str!("fixtures/yahoo/teams.json");
const PREDRAFT: &str = include_str!("fixtures/yahoo/draft_results_predraft.json");
const PARTIAL: &str = include_str!("fixtures/yahoo/draft_results_partial.json");
const COMPLETE: &str = include_str!("fixtures/yahoo/draft_results_complete.json");
const AUCTION: &str = include_str!("fixtures/yahoo/draft_results_auction.json");
const AUCTION_LEAGUE: &str = include_str!("fixtures/yahoo/league_settings_auction.json");
const KEEPERS: &str = include_str!("fixtures/yahoo/draft_results_keepers.json");
const PLAYERS_0: &str = include_str!("fixtures/yahoo/players_page_0.json");
const PLAYERS_1: &str = include_str!("fixtures/yahoo/players_page_1.json");
const ROSTER: &str = include_str!("fixtures/yahoo/team_roster.json");
const ROSTERS: &str = include_str!("fixtures/yahoo/teams_rosters.json");

#[test]
fn the_login_users_leagues_come_out_of_three_levels_of_wrapping() {
    let leagues = parse::user_leagues(&json(USER_LEAGUES));
    assert_eq!(leagues.len(), 2);
    assert_eq!(leagues[0].league_key, "449.l.12345");
    assert_eq!(leagues[0].league_id, "12345");
    assert_eq!(leagues[0].name, "Wire Wednesday");
    assert_eq!(leagues[0].season, "2026");
    assert_eq!(leagues[0].num_teams, 12);
    assert_eq!(leagues[0].draft_status, "predraft");
    assert_eq!(leagues[0].scoring_type.as_deref(), Some("head"));
    assert_eq!(leagues[1].league_key, "449.l.67890");
    assert_eq!(leagues[1].draft_status, "postdraft");
    assert_eq!(leagues[1].num_teams, 10);
}

#[test]
fn a_league_list_carries_no_settings_of_its_own() {
    // `/leagues` returns the league resource without `/settings`, so the
    // roster and scoring lists are empty until the league is asked for.
    let leagues = parse::user_leagues(&json(USER_LEAGUES));
    assert!(leagues[0].roster_positions.is_empty());
    assert!(leagues[0].stat_modifiers.is_empty());
}

#[test]
fn the_league_resource_parses_with_its_settings_folded_in() {
    let league = parse::league(&json(LEAGUE)).expect("a league");
    assert_eq!(league.league_key, "449.l.12345");
    assert_eq!(league.league_id, "12345");
    assert_eq!(league.name, "Wire Wednesday");
    assert_eq!(league.season, "2026");
    assert_eq!(league.num_teams, 12);
    assert_eq!(league.draft_status, "predraft");
    assert_eq!(league.draft_type.as_deref(), Some("live"));
    assert_eq!(league.scoring_type.as_deref(), Some("head"));
    assert_eq!(league.draft_time, Some(1_789_000_000));
}

#[test]
fn the_roster_slots_keep_yahoos_own_names_and_counts() {
    let league = parse::league(&json(LEAGUE)).expect("a league");
    let slots: Vec<(&str, u32)> = league
        .roster_positions
        .iter()
        .map(|slot| (slot.position.as_str(), slot.count))
        .collect();
    assert_eq!(
        slots,
        vec![
            ("QB", 1),
            ("WR", 2),
            ("RB", 2),
            ("TE", 1),
            ("W/R/T", 1),
            ("Q/W/R/T", 1),
            ("K", 1),
            ("DEF", 1),
            ("BN", 6),
            ("IR", 2),
        ]
    );
}

#[test]
fn the_stat_modifiers_come_out_as_numbers_though_yahoo_sent_strings() {
    let league = parse::league(&json(LEAGUE)).expect("a league");
    let by_id: std::collections::HashMap<u32, f64> = league
        .stat_modifiers
        .iter()
        .map(|modifier| (modifier.stat_id, modifier.value))
        .collect();
    assert_eq!(by_id.get(&4), Some(&0.04));
    assert_eq!(by_id.get(&5), Some(&4.0));
    assert_eq!(by_id.get(&6), Some(&-1.0));
    assert_eq!(by_id.get(&11), Some(&0.5));
    assert_eq!(by_id.get(&56), Some(&-4.0));
    assert_eq!(league.stat_modifiers.len(), 30);
}

#[test]
fn the_stat_categories_carry_both_names() {
    let league = parse::league(&json(LEAGUE)).expect("a league");
    assert_eq!(league.stat_categories.len(), 5);
    let passing = &league.stat_categories[0];
    assert_eq!(passing.stat_id, 4);
    assert_eq!(passing.name, "Passing Yards");
    assert_eq!(passing.display, "Pass Yds");
}

#[test]
fn the_teams_come_out_of_the_doubly_wrapped_attribute_lists() {
    let teams = parse::teams(&json(TEAMS));
    assert_eq!(teams.len(), 3);
    assert_eq!(teams[0].team_key, "449.l.12345.t.1");
    assert_eq!(teams[0].team_id, "1");
    assert_eq!(teams[0].name, "Ada's Autos");
    assert_eq!(teams[0].draft_position, Some(1));
    assert_eq!(teams[1].name, "Bo's Bots");
    assert_eq!(teams[1].draft_position, Some(2));
}

#[test]
fn the_logged_in_manager_is_the_one_flagged_current() {
    let teams = parse::teams(&json(TEAMS));
    let mine: Vec<&str> = teams
        .iter()
        .filter(|team| team.managers.iter().any(|manager| manager.is_current_login))
        .map(|team| team.team_key.as_str())
        .collect();
    assert_eq!(mine, vec!["449.l.12345.t.1"]);
    assert_eq!(teams[0].managers[0].nickname, "Ada");
    assert_eq!(teams[0].managers[0].guid, "WIREGUID000000000000000000");
}

#[test]
fn a_co_managed_team_keeps_both_managers() {
    let teams = parse::teams(&json(TEAMS));
    let nicknames: Vec<&str> = teams[1]
        .managers
        .iter()
        .map(|manager| manager.nickname.as_str())
        .collect();
    assert_eq!(nicknames, vec!["Bo", "Cy"]);
    assert!(teams[1].managers.iter().all(|m| !m.is_current_login));
}

#[test]
fn a_team_with_no_draft_position_yet_says_so() {
    let teams = parse::teams(&json(TEAMS));
    assert_eq!(teams[2].name, "Late Joiners");
    assert_eq!(teams[2].draft_position, None);
}

#[test]
fn a_draft_that_has_not_started_has_no_picks() {
    assert!(parse::draft_results(&json(PREDRAFT)).is_empty());
}

#[test]
fn a_draft_in_progress_gives_up_the_picks_made_so_far() {
    let picks = parse::draft_results(&json(PARTIAL));
    // Four rows, but the fourth is a slot Yahoo has recorded without a player.
    assert_eq!(picks.len(), 4);
    assert_eq!(picks[0].pick, 1);
    assert_eq!(picks[0].round, 1);
    assert_eq!(picks[0].team_key, "449.l.12345.t.1");
    assert_eq!(picks[0].player_key, "449.p.30977");
    assert_eq!(picks[2].pick, 3);
    assert_eq!(picks[3].pick, 4);
    assert!(picks[3].player_key.is_empty());
    assert!(picks.iter().all(|pick| pick.cost.is_none()));
}

#[test]
fn a_finished_draft_gives_up_every_pick_in_order() {
    let picks = parse::draft_results(&json(COMPLETE));
    assert_eq!(picks.len(), 6);
    let numbers: Vec<u32> = picks.iter().map(|pick| pick.pick).collect();
    assert_eq!(numbers, vec![1, 2, 3, 4, 5, 6]);
    let rounds: Vec<u32> = picks.iter().map(|pick| pick.round).collect();
    assert_eq!(rounds, vec![1, 1, 1, 2, 2, 2]);
    // A snake: round two runs back up the order.
    assert_eq!(picks[3].team_key, "449.l.12345.t.3");
    assert_eq!(picks[5].team_key, "449.l.12345.t.1");
}

#[test]
fn an_auction_carries_what_each_player_cost() {
    let picks = parse::draft_results(&json(AUCTION));
    assert_eq!(picks.len(), 3);
    assert_eq!(picks[0].cost, Some(55.0));
    assert_eq!(picks[1].cost, Some(41.0));
    assert_eq!(picks[2].cost, Some(1.0));
}

#[test]
fn a_page_of_players_parses_every_field_the_board_shows() {
    let page = parse::players(&json(PLAYERS_0));
    assert_eq!(page.count, 2);
    assert_eq!(page.players.len(), 2);
    let chase = &page.players[0];
    assert_eq!(chase.player_key, "449.p.30977");
    assert_eq!(chase.player_id, "30977");
    assert_eq!(chase.full_name, "Ja'Marr Chase");
    assert_eq!(chase.first, "Ja'Marr");
    assert_eq!(chase.last, "Chase");
    assert_eq!(chase.editorial_team_abbr, "Cin");
    assert_eq!(chase.display_position, "WR");
    assert_eq!(chase.eligible_positions, vec!["WR", "W/R/T"]);
    assert_eq!(chase.bye_week, Some(10));
    assert_eq!(chase.uniform_number.as_deref(), Some("1"));
    assert_eq!(chase.status, None);
}

#[test]
fn an_injury_designation_comes_through_on_the_player() {
    let page = parse::players(&json(PLAYERS_0));
    let bijan = &page.players[1];
    assert_eq!(bijan.full_name, "Bijan Robinson");
    assert_eq!(bijan.status.as_deref(), Some("Q"));
    assert_eq!(bijan.bye_week, Some(5));
}

#[test]
fn a_defence_has_no_bye_week_and_that_is_not_an_error() {
    let page = parse::players(&json(PLAYERS_1));
    assert_eq!(page.count, 1);
    let defence = &page.players[0];
    assert_eq!(defence.display_position, "DEF");
    assert_eq!(defence.bye_week, None);
    assert_eq!(defence.uniform_number, None);
    assert_eq!(defence.last, "");
}

#[test]
fn a_short_page_is_how_the_end_of_the_pool_is_recognised() {
    let full = parse::players(&json(PLAYERS_0));
    let last = parse::players(&json(PLAYERS_1));
    assert_eq!(full.players.len(), 2);
    assert!(last.players.len() < full.players.len());
}

#[test]
fn a_team_roster_parses_with_the_same_player_reader() {
    let roster = parse::players(&json(ROSTER));
    assert_eq!(roster.players.len(), 2);
    assert_eq!(roster.players[0].full_name, "Ja'Marr Chase");
    assert_eq!(roster.players[1].full_name, "Wire Kicker");
    assert_eq!(roster.players[1].display_position, "K");
    assert_eq!(roster.players[1].bye_week, Some(14));
}

#[test]
fn the_team_a_roster_belongs_to_is_still_readable_beside_it() {
    // The roster payload leads with the team's own attributes, so one call
    // answers both "who" and "what".
    let payload = json(ROSTER);
    let map = parse::flatten(parse::find(&payload, "team").expect("a team"));
    assert_eq!(parse::text(&map, "team_key"), "449.l.12345.t.1");
    assert_eq!(parse::text(&map, "name"), "Ada's Autos");
}

#[test]
fn an_auction_leagues_settings_carry_the_budget_the_bids_are_measured_against() {
    // The failure this prevents: the auction was detected and the budget was
    // not read, so nothing downstream could say what a $55 bid was a share of.
    let league = parse::league(&json(AUCTION_LEAGUE)).expect("the auction league");
    assert!(league.is_auction_draft);
    assert_eq!(league.draft_budget, Some(200));
    // Yahoo sends both of these as strings, like everything else here.
    assert_eq!(league.num_keepers, Some(2));
    assert_eq!(league.season, "2026");
    assert_eq!(league.roster_positions.len(), 8);
}

#[test]
fn a_league_that_is_neither_an_auction_nor_a_keeper_league_says_so_by_omission() {
    let league = parse::league(&json(LEAGUE)).expect("the plain league");
    assert!(!league.is_auction_draft);
    assert_eq!(league.draft_budget, None);
    assert_eq!(league.num_keepers, None);
}

#[test]
fn the_keeper_flag_on_a_draft_result_survives_the_parse() {
    // The failure this prevents: `is_keeper` was never read off a pick, so a
    // keeper league's first round looked like an ordinary one.
    let picks = parse::draft_results(&json(KEEPERS));
    assert_eq!(picks.len(), 3);
    assert_eq!(picks[0].is_keeper, Some(true));
    assert_eq!(picks[1].is_keeper, Some(false));
    assert_eq!(
        picks[2].is_keeper, None,
        "a pick Yahoo said nothing about must not read as 'not a keeper'"
    );
}

#[test]
fn the_keeper_flag_yahoo_sends_as_an_object_on_a_roster_row_is_read_too() {
    // On a roster Yahoo writes `is_keeper: {"status": .., "cost": .., "kept": "1"}`
    // rather than a plain 1, and reading that shape as a boolean gets nothing.
    let page = parse::players(&json(ROSTER));
    assert_eq!(page.players[0].is_keeper, Some(true));
    assert_eq!(
        page.players[1].is_keeper, None,
        "a player row without the field at all is not a keeper decision"
    );
}

#[test]
fn a_snake_leagues_picks_carry_no_keeper_opinion() {
    for pick in parse::draft_results(&json(PARTIAL)) {
        assert_eq!(pick.is_keeper, None, "pick {} invented a flag", pick.pick);
    }
}

#[test]
fn the_rosters_resource_names_every_teams_keepers_in_one_payload() {
    // The failure this prevents: the keeper flag was read off `draftresults`,
    // which the live resource does not send, so a keeper league's kept players
    // were drawn as ordinary picks. `teams;out=roster` always says.
    let rows = parse::rosters(&json(ROSTERS));
    assert_eq!(rows.len(), 3, "one roster per team was expected");
    let kept: Vec<(&str, Option<bool>)> = rows
        .iter()
        .map(|player| (player.player_key.as_str(), player.is_keeper))
        .collect();
    assert_eq!(
        kept[0],
        ("449.p.30977", Some(false)),
        "Yahoo's empty keeper object means 'this league keeps players and he is not one'"
    );
    assert_eq!(
        kept[1],
        ("449.p.31883", None),
        "a row with no keeper field at all is not a decision either way"
    );
    assert_eq!(kept[2], ("449.p.100002", Some(true)));
    // The rows are whole players, not keeper flags with a key attached: the
    // same reader builds them, so a name and a position come free.
    assert_eq!(rows[0].full_name, "Ja'Marr Chase");
    assert_eq!(rows[0].display_position, "WR");
}
