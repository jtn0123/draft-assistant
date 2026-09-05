//! Auction money and keeper flags, which arrive on different resources and
//! used to be dropped on the floor between them.

use super::*;
use crate::yahoo_types::{YahooDraftPick, YahooLeague, YahooPlayer, YahooTeam};

fn team(key: &str, position: Option<u32>) -> YahooTeam {
    YahooTeam {
        team_key: key.into(),
        team_id: key.rsplit('.').next().unwrap_or("0").into(),
        name: format!("Team {key}"),
        draft_position: position,
        ..YahooTeam::default()
    }
}

fn result(pick: u32, player_key: &str) -> YahooDraftPick {
    YahooDraftPick {
        pick,
        round: 1,
        team_key: "449.l.1.t.1".into(),
        player_key: player_key.into(),
        ..YahooDraftPick::default()
    }
}

fn pool(rows: &[(&str, Option<bool>)]) -> HashMap<String, YahooPlayer> {
    rows.iter()
        .map(|(key, keeper)| {
            (
                (*key).to_string(),
                YahooPlayer {
                    player_key: (*key).to_string(),
                    display_position: "WR".into(),
                    is_keeper: *keeper,
                    ..YahooPlayer::default()
                },
            )
        })
        .collect()
}

#[test]
fn an_auction_league_carries_its_budget_beside_the_costs() {
    // The failure this prevents: the costs were parsed and the budget was
    // not, so a $55 bid had nothing to be measured against and the auction
    // board could not say whether it was cheap.
    let league = YahooLeague {
        is_auction_draft: true,
        draft_budget: Some(200),
        ..YahooLeague::default()
    };
    let results = vec![
        YahooDraftPick {
            cost: Some(55.0),
            ..result(1, "449.p.100")
        },
        YahooDraftPick {
            cost: Some(1.0),
            ..result(2, "449.p.200")
        },
    ];
    let auction = auction(&league, &results);
    assert_eq!(auction.budget, Some(200));
    assert_eq!(auction.costs.get("yahoo:100"), Some(&55.0));
    assert_eq!(auction.costs.get("yahoo:200"), Some(&1.0));
}

#[test]
fn a_snake_draft_has_no_auction_costs() {
    assert!(auction_costs(&[result(1, "449.p.100")]).is_empty());
}

#[test]
fn an_auction_keeps_what_each_player_went_for() {
    let costs = auction_costs(&[
        YahooDraftPick {
            cost: Some(55.0),
            ..result(1, "449.p.100")
        },
        YahooDraftPick {
            cost: Some(1.0),
            ..result(2, "449.p.200")
        },
    ]);
    assert_eq!(costs.get("yahoo:100"), Some(&55.0));
    assert_eq!(costs.get("yahoo:200"), Some(&1.0));
}

#[test]
fn a_snake_league_reports_no_budget_and_no_costs() {
    let auction = auction(&YahooLeague::default(), &[result(1, "449.p.100")]);
    assert_eq!(auction.budget, None);
    assert!(auction.costs.is_empty());
}

#[test]
fn a_keeper_yahoo_flagged_on_the_pick_reaches_the_board_as_one() {
    // The failure this prevents: every Yahoo pick arrived with `is_keeper:
    // None`, so a kept player was drawn as an ordinary first-round pick and
    // the round he really cost was never accounted for.
    let picks = picks(
        &[
            YahooDraftPick {
                is_keeper: Some(true),
                ..result(1, "449.p.100")
            },
            YahooDraftPick {
                is_keeper: Some(false),
                ..result(2, "449.p.200")
            },
            result(3, "449.p.300"),
        ],
        &[team("449.l.1.t.1", Some(1))],
        &HashMap::new(),
    );
    assert_eq!(picks[0].is_keeper, Some(true));
    assert_eq!(picks[1].is_keeper, Some(false));
    assert_eq!(
        picks[2].is_keeper, None,
        "silence from Yahoo must stay silence, or the app's own keeper test \
         never gets a turn"
    );
}

#[test]
fn a_keeper_yahoo_flagged_on_the_roster_instead_is_read_from_there() {
    let picks = picks(
        &[result(1, "449.p.100"), result(2, "449.p.200")],
        &[team("449.l.1.t.1", Some(1))],
        &pool(&[("449.p.100", Some(true)), ("449.p.200", None)]),
    );
    assert_eq!(picks[0].is_keeper, Some(true));
    assert_eq!(picks[1].is_keeper, None);
}

#[test]
fn the_pick_wins_over_the_roster_when_both_have_an_opinion() {
    // The draft result is the record of what happened in this draft; the
    // roster flag is about the roster as it stands now.
    let picks = picks(
        &[YahooDraftPick {
            is_keeper: Some(true),
            ..result(1, "449.p.100")
        }],
        &[team("449.l.1.t.1", Some(1))],
        &pool(&[("449.p.100", Some(false))]),
    );
    assert_eq!(picks[0].is_keeper, Some(true));
}

#[test]
fn a_return_touchdown_pays_the_returner_rather_than_a_defence() {
    // Yahoo's stat 15 is a player's own return TD. Sleeper spells that
    // `st_td`; `def_st_td`, `def_kr_td` and `def_pr_td` are the defence
    // unit's, and paying id 15 out of one of those would credit the score to
    // every defence in the league and to none of the returners.
    let scoring = scoring_settings(&[crate::yahoo_types::StatModifier {
        stat_id: 15,
        value: 6.0,
    }]);
    assert_eq!(scoring.get("st_td"), Some(&6.0));
    for defence in ["def_st_td", "def_kr_td", "def_pr_td", "def_td"] {
        assert_eq!(scoring.get(defence), None, "{defence} was paid for id 15");
    }
    assert_eq!(scoring.len(), 1);
}
