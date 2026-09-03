//! Clean shapes for the handful of Yahoo Fantasy resources this app reads.
//!
//! Yahoo's JSON is a transliteration of its XML: every resource is a list of
//! single-key objects (`[{"league_key": ".."}, {"name": ".."}]`), collections
//! are objects keyed by the numeric string `"0"`, `"1"`, ... plus a `count`,
//! and a few resources wrap their attribute list in a second array
//! (`"team": [[{..}, {..}], {..}]`). None of that belongs above the client, so
//! [`crate::yahoo_parse`] flattens it onto the structs declared here.
//!
//! Everything is optional-tolerant on purpose: Yahoo omits fields per league
//! (no `cost` outside auctions, no `bye_weeks` for a defence, no
//! `draft_position` before the order is set), and a missing field must never
//! cost us the rest of the payload.

use serde::{Deserialize, Serialize};

/// One roster slot line from league settings: `QB` x1, `W/R/T` x1, `BN` x6.
/// Yahoo's own position names, untranslated — [`crate::yahoo_map`] is where
/// they become the app's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterSlot {
    pub position: String,
    pub count: u32,
}

/// A scoring rule: how many points one unit of `stat_id` is worth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatModifier {
    pub stat_id: u32,
    pub value: f64,
}

/// A stat the league scores, for display. `display` is Yahoo's short name
/// ("Pass Yds"); `name` is the long one ("Passing Yards").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatCategory {
    pub stat_id: u32,
    pub name: String,
    pub display: String,
}

/// A league, with its settings folded in when they were asked for.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct YahooLeague {
    /// `449.l.12345` — the id every other call takes.
    pub league_key: String,
    pub league_id: String,
    pub name: String,
    pub season: String,
    pub num_teams: u32,
    /// `predraft` | `draft` | `postdraft`.
    pub draft_status: String,
    /// Epoch seconds, as a string in the payload. `None` until scheduled.
    pub draft_time: Option<u64>,
    /// `live` | `auction` | `offline` (absent on some leagues).
    pub draft_type: Option<String>,
    /// `head` | `roto` | `point`.
    pub scoring_type: Option<String>,
    pub roster_positions: Vec<RosterSlot>,
    pub stat_modifiers: Vec<StatModifier>,
    pub stat_categories: Vec<StatCategory>,
}

/// One manager of a team. Co-managed teams have more than one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct YahooManager {
    pub guid: String,
    pub nickname: String,
    /// True for the manager whose token made the call — how "my team" is found.
    pub is_current_login: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct YahooTeam {
    /// `449.l.12345.t.7`.
    pub team_key: String,
    pub team_id: String,
    pub name: String,
    pub managers: Vec<YahooManager>,
    /// 1-based draft slot. Absent before the commissioner sets the order.
    pub draft_position: Option<u32>,
}

/// One pick from `league/<key>/draftresults`. During a live draft the list
/// holds only the picks made so far, which is exactly what the poller wants.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct YahooDraftPick {
    /// Overall pick number, 1-based.
    pub pick: u32,
    pub round: u32,
    pub team_key: String,
    /// `449.p.30977`. Empty on a pick Yahoo has recorded but not filled.
    pub player_key: String,
    /// Auction drafts only.
    pub cost: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct YahooPlayer {
    pub player_key: String,
    pub player_id: String,
    pub full_name: String,
    pub first: String,
    pub last: String,
    /// Yahoo writes these mixed-case ("Cin"); the mapper uppercases.
    pub editorial_team_abbr: String,
    pub display_position: String,
    pub eligible_positions: Vec<String>,
    /// `Q`, `O`, `IR`, `NA`, ... Absent for a healthy player.
    pub status: Option<String>,
    pub bye_week: Option<u32>,
    pub uniform_number: Option<String>,
}

/// A page of `league/<key>/players`: the rows plus what the caller needs to
/// decide whether to ask for another page.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerPage {
    pub players: Vec<YahooPlayer>,
    /// Yahoo's `count` for this page. The last page is the one that comes
    /// back short of the requested `count` (Yahoo has no total).
    pub count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_empty_rather_than_absent() {
        let league = YahooLeague::default();
        assert!(league.league_key.is_empty());
        assert!(league.roster_positions.is_empty());
        assert!(league.draft_time.is_none());
    }

    #[test]
    fn a_league_round_trips_through_serde() {
        let league = YahooLeague {
            league_key: "449.l.1".into(),
            num_teams: 12,
            roster_positions: vec![RosterSlot {
                position: "W/R/T".into(),
                count: 1,
            }],
            stat_modifiers: vec![StatModifier {
                stat_id: 4,
                value: 0.04,
            }],
            ..YahooLeague::default()
        };
        let text = serde_json::to_string(&league).expect("serialize");
        let back: YahooLeague = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back, league);
    }

    #[test]
    fn a_pick_without_a_cost_is_not_an_auction_pick() {
        let pick = YahooDraftPick {
            pick: 1,
            round: 1,
            ..YahooDraftPick::default()
        };
        assert!(pick.cost.is_none());
    }
}
