//! The stub Sleeper that `command_flows.rs` runs against: one league, one
//! draft, one account, and every endpoint the loaders touch for them.

use crate::stub;

/// A Sleeper league id is a long run of digits, and `add_league` insists on
/// one, so the fixture uses a realistic id rather than "league-1".
pub const LEAGUE_ID: &str = "1000000000000000001";
pub const DRAFT_ID: &str = "2000000000000000002";
pub const USER_ID: &str = "3000000000000000003";

fn league_json() -> String {
    format!(
        r#"{{"league_id": "{LEAGUE_ID}", "name": "Command League", "season": "2026",
             "status": "drafting", "total_rosters": 2,
             "roster_positions": ["QB", "RB", "WR", "TE", "FLEX", "BN"],
             "scoring_settings": {{"rec": 1.0, "rush_yd": 0.1, "rush_td": 6.0,
                                   "rec_yd": 0.1, "rec_td": 6.0, "pass_yd": 0.04,
                                   "pass_td": 4.0}},
             "draft_id": "{DRAFT_ID}", "settings": {{"playoff_week_start": 15}}}}"#
    )
}

const PLAYERS: &str = r#"{
    "qb-1": {"full_name": "Command Passer", "position": "QB", "team": "AAA"},
    "rb-1": {"full_name": "Command Runner", "position": "RB", "team": "BBB"},
    "wr-1": {"full_name": "Command Catcher", "position": "WR", "team": "CCC"}
}"#;

const SEASON_ROWS: &str = r#"[
    {"player_id": "qb-1", "stats": {"pass_yd": 4200.0, "pass_td": 32.0, "adp_ppr": 14.0},
     "player": {"position": "QB", "team": "AAA"}},
    {"player_id": "rb-1", "stats": {"rush_yd": 1200.0, "rush_td": 10.0, "rec": 40.0, "adp_ppr": 3.0},
     "player": {"position": "RB", "team": "BBB"}},
    {"player_id": "wr-1", "stats": {"rec_yd": 1300.0, "rec_td": 9.0, "rec": 95.0, "adp_ppr": 5.0},
     "player": {"position": "WR", "team": "CCC"}}
]"#;

const ROSTERS: &str = r#"[
    {"roster_id": 1, "owner_id": "3000000000000000003", "players": ["qb-1", "rb-1"],
     "starters": ["qb-1"], "settings": {"wins": 1, "losses": 0, "fpts": 120}},
    {"roster_id": 2, "owner_id": "other", "players": ["wr-1"],
     "starters": ["wr-1"], "settings": {"wins": 0, "losses": 1, "fpts": 90}}
]"#;

pub fn route(path: &str) -> Option<stub::Reply> {
    let path = path.split('?').next().unwrap_or(path);
    let ok = |body: String| Some((200u16, body));
    if path == "/v1/players/nfl" {
        return ok(PLAYERS.to_string());
    }
    if path == "/v1/state/nfl" {
        return ok(r#"{"season": "2026", "week": 1, "display_week": 1}"#.to_string());
    }
    if path.starts_with("/scores/nfl/") {
        return ok("[]".to_string());
    }
    if let Some(rest) = path.strip_prefix("/projections/nfl/2026") {
        return match rest.is_empty() {
            true => ok(SEASON_ROWS.to_string()),
            false => ok("[]".to_string()),
        };
    }
    if let Some(rest) = path.strip_prefix(&format!("/v1/league/{LEAGUE_ID}")) {
        return match rest {
            "" => ok(league_json()),
            "/users" => ok(format!(
                r#"[{{"user_id": "{USER_ID}", "display_name": "Ada"}}]"#
            )),
            "/rosters" => ok(ROSTERS.to_string()),
            "/winners_bracket" => ok("[]".to_string()),
            _ if rest.starts_with("/matchups") || rest.starts_with("/transactions") => {
                ok("[]".to_string())
            }
            _ => None,
        };
    }
    if let Some(rest) = path.strip_prefix(&format!("/v1/draft/{DRAFT_ID}")) {
        return match rest {
            "" => ok(format!(
                r#"{{"draft_id": "{DRAFT_ID}", "status": "drafting", "type": "snake",
                     "settings": {{"teams": 2, "rounds": 3}},
                     "draft_order": {{"{USER_ID}": 1}}, "season": "2026"}}"#
            )),
            "/picks" | "/traded_picks" => ok("[]".to_string()),
            _ => None,
        };
    }
    match path {
        "/v1/user/ada" => ok(format!(
            r#"{{"user_id": "{USER_ID}", "username": "ada", "display_name": "Ada"}}"#
        )),
        _ if path.starts_with(&format!("/v1/user/{USER_ID}/leagues/nfl/")) => {
            ok(format!("[{}]", league_json()))
        }
        _ => None,
    }
}
