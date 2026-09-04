//! The four primitives every Yahoo parser is built from: [`flatten`],
//! [`items`], [`find`] and the scalar readers. The struct-level parses are
//! driven off real-shaped fixtures in `tests/yahoo_parse.rs`.

use super::*;
use serde_json::json;

#[test]
fn a_list_of_one_key_objects_flattens_into_a_map() {
    let attrs = json!([{"league_key": "449.l.1"}, {"name": "Wire"}, {"num_teams": 12}]);
    let map = flatten(&attrs);
    assert_eq!(text(&map, "league_key"), "449.l.1");
    assert_eq!(text(&map, "name"), "Wire");
    assert_eq!(num::<u32>(&map, "num_teams"), 12);
}

#[test]
fn the_doubly_wrapped_attribute_list_flattens_too() {
    // `"team": [[{..}, {..}], {..}]` -- the shape teams and players arrive in.
    let attrs = json!([[{"team_key": "449.l.1.t.2"}, {"name": "Bo's Bots"}], {"team_points": {"total": "0"}}]);
    let map = flatten(&attrs);
    assert_eq!(text(&map, "team_key"), "449.l.1.t.2");
    assert_eq!(text(&map, "name"), "Bo's Bots");
    assert!(map.contains_key("team_points"));
}

#[test]
fn the_first_value_for_a_repeated_key_wins() {
    // Yahoo repeats `count` at several depths; the outer one is the resource's.
    let attrs = json!([{"count": 3}, {"count": 9}]);
    assert_eq!(num::<u32>(&flatten(&attrs), "count"), 3);
}

#[test]
fn flattening_something_that_is_not_a_list_is_empty_rather_than_a_panic() {
    assert!(flatten(&json!("nope")).is_empty());
    assert!(flatten(&json!(null)).is_empty());
}

#[test]
fn a_numeric_keyed_collection_is_walked_in_order() {
    let collection = json!({
        "0": {"team": {"id": 1}},
        "1": {"team": {"id": 2}},
        "2": {"team": {"id": 3}},
        "count": 3
    });
    let ids: Vec<i64> = items(&collection, "team")
        .into_iter()
        .filter_map(|team| team.get("id")?.as_i64())
        .collect();
    assert_eq!(ids, vec![1, 2, 3]);
}

#[test]
fn ten_members_sort_by_number_and_not_by_string() {
    let mut collection = serde_json::Map::new();
    for index in 0..12 {
        collection.insert(index.to_string(), json!({"team": {"id": index}}));
    }
    collection.insert("count".into(), json!(12));
    let ids: Vec<i64> = items(&Value::Object(collection), "team")
        .into_iter()
        .filter_map(|team| team.get("id")?.as_i64())
        .collect();
    assert_eq!(ids, (0..12).collect::<Vec<_>>());
}

#[test]
fn a_plain_array_collection_works_the_same_way() {
    let collection = json!([{"position": "WR"}, {"position": "W/R/T"}]);
    let positions: Vec<&str> = items(&collection, "position")
        .into_iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(positions, vec!["WR", "W/R/T"]);
}

#[test]
fn a_member_missing_the_key_is_skipped_rather_than_defaulted() {
    let collection = json!({"0": {"team": 1}, "1": {"not_a_team": 2}, "count": 2});
    assert_eq!(items(&collection, "team").len(), 1);
}

#[test]
fn find_reaches_a_collection_however_deep_yahoo_buried_it() {
    let payload = json!({
        "fantasy_content": {"league": [{"league_key": "449.l.1"}, {"draftresults": {"count": 0}}]}
    });
    assert!(find(&payload, "draftresults").is_some());
    assert!(find(&payload, "league").is_some());
    assert!(find(&payload, "transactions").is_none());
}

#[test]
fn find_prefers_the_key_on_the_object_it_is_looking_at() {
    let payload = json!({"players": {"outer": true}, "wrap": {"players": {"outer": false}}});
    assert_eq!(
        find(&payload, "players").and_then(|p| p.get("outer")),
        Some(&json!(true))
    );
}

#[test]
fn numbers_are_read_whether_yahoo_sent_them_quoted_or_not() {
    let map = flatten(&json!([{"a": "12"}, {"b": 12}, {"c": "0.04"}, {"d": 0.5}]));
    assert_eq!(opt_num::<u32>(&map, "a"), Some(12));
    assert_eq!(opt_num::<u32>(&map, "b"), Some(12));
    assert_eq!(opt_num::<f64>(&map, "c"), Some(0.04));
    assert_eq!(opt_num::<f64>(&map, "d"), Some(0.5));
}

#[test]
fn an_empty_string_is_absence_not_an_empty_answer() {
    let map = flatten(&json!([{"draft_time": ""}, {"name": "Wire"}]));
    assert_eq!(opt_text(&map, "draft_time"), None);
    assert_eq!(opt_num::<u64>(&map, "draft_time"), None);
    assert_eq!(opt_text(&map, "name").as_deref(), Some("Wire"));
}

#[test]
fn a_missing_number_defaults_rather_than_failing_the_payload() {
    let map = flatten(&json!([{"name": "Wire"}]));
    assert_eq!(num::<u32>(&map, "num_teams"), 0);
    assert_eq!(text(&map, "league_key"), "");
}

#[test]
fn a_field_of_the_wrong_type_is_read_as_absent() {
    let map = flatten(&json!([{"name": {"full": "nested"}}, {"num_teams": [12]}]));
    assert_eq!(opt_text(&map, "name"), None);
    assert_eq!(opt_num::<u32>(&map, "num_teams"), None);
}

#[test]
fn every_way_yahoo_writes_a_boolean_reads_as_true() {
    let map = flatten(&json!([{"a": "1"}, {"b": 1}, {"c": true}, {"d": "true"}]));
    for key in ["a", "b", "c", "d"] {
        assert!(flag(&map, key), "{key} should be true");
    }
}

#[test]
fn everything_else_reads_as_false() {
    let map = flatten(&json!([{"a": "0"}, {"b": 0}, {"c": false}, {"d": "no"}]));
    for key in ["a", "b", "c", "d", "missing"] {
        assert!(!flag(&map, key), "{key} should be false");
    }
}

#[test]
fn a_payload_with_no_collection_parses_to_nothing_rather_than_failing() {
    let empty = json!({"fantasy_content": {}});
    assert!(teams(&empty).is_empty());
    assert!(draft_results(&empty).is_empty());
    assert!(user_leagues(&empty).is_empty());
    assert!(league(&empty).is_none());
    assert_eq!(players(&empty), crate::yahoo_types::PlayerPage::default());
}

#[test]
fn a_league_with_no_key_is_not_a_league() {
    let payload = json!({"fantasy_content": {"league": [{"name": "Nameless"}]}});
    assert!(league(&payload).is_none());
}

#[test]
fn yahoos_auction_flag_is_read_off_the_settings_however_it_is_written() {
    for written in [json!("1"), json!(1), json!(true)] {
        let payload = json!({"fantasy_content": {"league": [
            {"league_key": "449.l.1"},
            {"settings": [{"draft_type": "live", "is_auction_draft": written}]}
        ]}});
        let parsed = league(&payload).expect("a league");
        assert!(
            parsed.is_auction_draft,
            "{written} should read as an auction"
        );
        // The type Yahoo actually sends for a live auction is kept as it is.
        assert_eq!(parsed.draft_type.as_deref(), Some("live"));
    }
    let snake = json!({"fantasy_content": {"league": [
        {"league_key": "449.l.1"},
        {"settings": [{"draft_type": "live", "is_auction_draft": "0"}]}
    ]}});
    assert!(!league(&snake).expect("a league").is_auction_draft);
    // Absent entirely — some older leagues omit it — is not an auction.
    let silent = json!({"fantasy_content": {"league": [
        {"league_key": "449.l.1"},
        {"settings": [{"draft_type": "live"}]}
    ]}});
    assert!(!league(&silent).expect("a league").is_auction_draft);
}
