use super::{cache_name, current_login, draft_for, picks_for, rounds_from, team_names};
use crate::yahoo_crosswalk;
use crate::yahoo_types::{
    RosterSlot, YahooDraftPick, YahooLeague, YahooManager, YahooPlayer, YahooTeam,
};
use std::collections::HashMap;

fn slots(pairs: &[(&str, u32)]) -> Vec<RosterSlot> {
    pairs
        .iter()
        .map(|(position, count)| RosterSlot {
            position: (*position).to_string(),
            count: *count,
        })
        .collect()
}

fn team(id: u32, name: &str, position: Option<u32>, mine: bool) -> YahooTeam {
    YahooTeam {
        team_key: format!("449.l.12345.t.{id}"),
        team_id: id.to_string(),
        name: name.to_string(),
        managers: vec![YahooManager {
            guid: format!("GUID{id}"),
            nickname: format!("Manager {id}"),
            is_current_login: mine,
        }],
        draft_position: position,
    }
}

fn league() -> YahooLeague {
    YahooLeague {
        league_key: "449.l.12345".into(),
        league_id: "12345".into(),
        name: "Wire Wednesday".into(),
        season: "2026".into(),
        num_teams: 3,
        draft_status: "draft".into(),
        draft_time: Some(1_789_000_000),
        draft_type: Some("live".into()),
        is_auction_draft: false,
        scoring_type: Some("head".into()),
        roster_positions: slots(&[("QB", 1), ("RB", 2), ("W/R/T", 1), ("BN", 5), ("IR", 2)]),
        stat_modifiers: vec![],
        stat_categories: vec![],
    }
}

#[test]
fn a_cache_file_is_named_for_its_league_and_cannot_walk_out_of_the_directory() {
    assert_eq!(
        cache_name("449.l.12345", "teams"),
        "yahoo_449_l_12345_teams.json"
    );
    assert_eq!(
        cache_name("../../etc/passwd", "league"),
        "yahoo_______etc_passwd_league.json"
    );
    // Two leagues never share a file.
    assert_ne!(
        cache_name("449.l.1", "players"),
        cache_name("449.l.2", "players")
    );
}

#[test]
fn the_rounds_are_the_seats_that_get_drafted_into() {
    // QB 1 + RB 2 + flex 1 + bench 5 = 9; the two IR slots are not drafted.
    assert_eq!(rounds_from(&league().roster_positions), 9);
    assert_eq!(rounds_from(&[]), 0);
}

#[test]
fn a_synthesized_draft_carries_the_order_the_status_and_the_shape() {
    let teams = vec![
        team(1, "Ada's Autos", Some(1), true),
        team(2, "Bob's Bots", Some(2), false),
        team(3, "Cy's Cars", Some(3), false),
    ];
    let draft = draft_for("449.l.12345", &league(), &teams);
    assert_eq!(draft.draft_id, "449.l.12345");
    assert_eq!(draft.status, "drafting");
    assert_eq!(draft.draft_type, "snake");
    assert_eq!(draft.settings.teams, 3);
    assert_eq!(draft.settings.rounds, 9);
    assert_eq!(draft.season.as_deref(), Some("2026"));
    // Yahoo's draft time is in seconds; Sleeper's start time in milliseconds.
    assert_eq!(draft.start_time, Some(1_789_000_000_000));
    let order = draft.draft_order.expect("the order is known");
    assert_eq!(order.get("449.l.12345.t.2"), Some(&2));
    assert_eq!(order.len(), 3);
}

#[test]
fn a_team_with_no_draft_position_yet_is_left_out_of_the_order() {
    let teams = vec![team(1, "Ada's Autos", None, true)];
    let draft = draft_for("449.l.12345", &league(), &teams);
    assert!(draft.draft_order.expect("an order, even empty").is_empty());
}

#[test]
fn every_yahoo_draft_status_lands_somewhere_the_board_understands() {
    let cases = [
        ("predraft", "pre_draft"),
        ("draft", "drafting"),
        ("postdraft", "complete"),
        ("something new", "pre_draft"),
    ];
    for (yahoo, expected) in cases {
        let mut league = league();
        league.draft_status = yahoo.to_string();
        assert_eq!(
            draft_for("449.l.1", &league, &[]).status,
            expected,
            "{yahoo}"
        );
    }
}

#[test]
fn an_auction_league_is_marked_as_one() {
    let mut league = league();
    league.draft_type = Some("auction".into());
    assert_eq!(draft_for("449.l.1", &league, &[]).draft_type, "auction");
}

#[test]
fn a_live_auction_is_an_auction_even_though_yahoo_calls_the_type_live() {
    // The shape Yahoo actually sends for an auction: the type says `live` and
    // only `is_auction_draft` says auction.
    let settings = serde_json::json!({
        "fantasy_content": { "league": [
            { "league_key": "449.l.12345", "league_id": "12345", "name": "Bid Night",
              "season": "2026", "num_teams": 12, "draft_status": "predraft" },
            { "settings": [ { "draft_type": "live", "is_auction_draft": "1" } ] }
        ] }
    });
    let parsed = crate::yahoo_parse::league(&settings).expect("the league parses");
    assert_eq!(parsed.draft_type.as_deref(), Some("live"));
    assert!(parsed.is_auction_draft);
    assert_eq!(draft_for("449.l.12345", &parsed, &[]).draft_type, "auction");
}

#[test]
fn the_snake_fixture_is_still_a_snake_once_the_auction_flag_is_read() {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/yahoo/league_settings.json"),
    )
    .expect("the settings fixture");
    let value: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    let parsed = crate::yahoo_parse::league(&value).expect("the league parses");
    // `is_auction_draft: "0"` is in the fixture and has to read as false.
    assert!(!parsed.is_auction_draft);
    assert_eq!(draft_for("449.l.12345", &parsed, &[]).draft_type, "snake");
}

#[test]
fn the_teams_name_the_slots_and_the_logged_in_manager_names_mine() {
    let teams = vec![
        team(1, "Ada's Autos", Some(1), false),
        team(2, "Bob's Bots", Some(2), true),
    ];
    let names = team_names(&teams);
    assert_eq!(
        names.get("449.l.12345.t.1").map(String::as_str),
        Some("Ada's Autos")
    );
    assert_eq!(
        current_login(&teams).map(|team| team.draft_position),
        Some(Some(2))
    );
    // Nobody flagged: the app has no team here, which is a legitimate state
    // for a league the user only watches.
    let watched = vec![team(1, "Ada's Autos", Some(1), false)];
    assert!(current_login(&watched).is_none());
}

fn yahoo_player(key: &str, full: &str, team: &str, position: &str) -> YahooPlayer {
    let mut parts = full.splitn(2, ' ');
    YahooPlayer {
        player_key: key.to_string(),
        player_id: key.rsplit('.').next().unwrap_or(key).to_string(),
        full_name: full.to_string(),
        first: parts.next().unwrap_or_default().to_string(),
        last: parts.next().unwrap_or_default().to_string(),
        editorial_team_abbr: team.to_string(),
        display_position: position.to_string(),
        eligible_positions: vec![position.to_string()],
        status: None,
        bye_week: None,
        uniform_number: None,
    }
}

#[test]
fn a_pick_names_the_player_by_the_id_the_board_put_him_on() {
    let pool = vec![
        yahoo_player("449.p.30977", "Ja'Marr Chase", "Cin", "WR"),
        yahoo_player("449.p.99999", "Nobody Atall", "Was", "TE"),
    ];
    let sleeper: HashMap<String, crate::sleeper::PlayerMeta> = [(
        "6794".to_string(),
        crate::sleeper::PlayerMeta {
            full_name: Some("Ja'Marr Chase".into()),
            first_name: Some("Ja'Marr".into()),
            last_name: Some("Chase".into()),
            position: Some("WR".into()),
            team: Some("CIN".into()),
            fantasy_positions: Some(vec!["WR".into()]),
            injury_status: None,
            years_exp: None,
            age: None,
        },
    )]
    .into_iter()
    .collect();
    let crosswalk = yahoo_crosswalk::build(&crate::yahoo_map::players(&pool), &sleeper);
    let teams = vec![
        team(1, "Ada's Autos", Some(1), true),
        team(2, "Bob", Some(2), false),
    ];
    let results = vec![
        YahooDraftPick {
            pick: 1,
            round: 1,
            team_key: "449.l.12345.t.1".into(),
            player_key: "449.p.30977".into(),
            cost: None,
        },
        YahooDraftPick {
            pick: 2,
            round: 1,
            team_key: "449.l.12345.t.2".into(),
            player_key: "449.p.99999".into(),
            cost: None,
        },
        // Recorded but not filled: not a pick yet.
        YahooDraftPick {
            pick: 3,
            round: 1,
            team_key: "449.l.12345.t.2".into(),
            player_key: String::new(),
            cost: None,
        },
    ];
    let picks = picks_for(&results, &teams, &pool, &crosswalk);
    assert_eq!(picks.len(), 2);
    // Matched: the Sleeper id, so the board and the projections find him.
    assert_eq!(picks[0].player_id, "6794");
    assert_eq!(picks[0].draft_slot, 1);
    // Unmatched: still a pick, still leaves the board, under his Yahoo id.
    assert_eq!(picks[1].player_id, "yahoo:99999");
    assert_eq!(picks[1].draft_slot, 2);
}
