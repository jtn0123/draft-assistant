//! Turning Yahoo's XML-shaped JSON into [`crate::yahoo_types`] structs.
//!
//! Three quirks drive everything here:
//!
//! 1. A resource's attributes arrive as a *list of one-key objects* rather
//!    than one object: `[{"league_key": "449.l.1"}, {"name": "Wire"}]`.
//!    [`flatten`] merges such a list into a map.
//! 2. A collection is an object whose keys are the numeric strings `"0"`,
//!    `"1"`, ... plus a `count`. [`items`] walks one in order, and also
//!    accepts the plain array Yahoo uses for the smaller lists.
//! 3. Some resources wrap the attribute list in a second array —
//!    `"team": [[{..}, {..}], {..}]` — and bury the interesting collection
//!    several levels down. [`flatten`] recurses through the arrays, and
//!    [`find`] does a depth-first search for a key wherever it is.
//!
//! Everything is tolerant: an absent or unexpectedly-typed field yields the
//! default for its slot instead of failing the whole payload. Yahoo varies
//! these per league (an auction has `cost`, a defence has no `bye_weeks`), and
//! losing an entire draft board over one missing key would be the wrong trade.

use crate::yahoo_types::{
    PlayerPage, RosterSlot, StatCategory, StatModifier, YahooDraftPick, YahooLeague, YahooManager,
    YahooPlayer, YahooTeam,
};
use serde_json::{Map, Value};

/// Merge a Yahoo attribute list into one map. Arrays are flattened
/// recursively; the first value seen for a key wins, which matters because
/// Yahoo repeats `count` and the odd id at several depths.
pub fn flatten(value: &Value) -> Map<String, Value> {
    let mut out = Map::new();
    flatten_into(value, &mut out);
    out
}

fn flatten_into(value: &Value, out: &mut Map<String, Value>) {
    match value {
        Value::Array(list) => {
            for item in list {
                flatten_into(item, out);
            }
        }
        Value::Object(map) => {
            for (key, inner) in map {
                if !out.contains_key(key) {
                    out.insert(key.clone(), inner.clone());
                }
            }
        }
        _ => {}
    }
}

/// The members of a Yahoo collection, in order.
///
/// Handles both shapes: the numeric-keyed object (`{"0": {"team": ...},
/// "count": 2}`) and the plain array (`[{"position": "WR"}]`). Each member is
/// unwrapped by `key`; a member that does not carry that key is skipped.
pub fn items<'a>(value: &'a Value, key: &str) -> Vec<&'a Value> {
    match value {
        Value::Array(list) => list.iter().filter_map(|item| item.get(key)).collect(),
        Value::Object(map) => {
            let mut numbered: Vec<(u64, &Value)> = map
                .iter()
                .filter_map(|(name, inner)| Some((name.parse::<u64>().ok()?, inner)))
                .collect();
            numbered.sort_by_key(|(index, _)| *index);
            numbered
                .into_iter()
                .filter_map(|(_, inner)| inner.get(key))
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Depth-first search for the first value stored under `key`.
///
/// Yahoo nests the thing you asked for under the resource you asked it about
/// (`fantasy_content.league[1].draftresults`), and the depth differs between
/// endpoints. Searching for the collection by name is steadier than spelling
/// out a path per endpoint.
pub fn find<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => {
            if let Some(found) = map.get(key) {
                return Some(found);
            }
            map.values().find_map(|inner| find(inner, key))
        }
        Value::Array(list) => list.iter().find_map(|item| find(item, key)),
        _ => None,
    }
}

/// A field as text. Yahoo sends ids and counts as strings in some payloads and
/// as numbers in others, so both are accepted.
pub fn text(map: &Map<String, Value>, key: &str) -> String {
    opt_text(map, key).unwrap_or_default()
}

pub fn opt_text(map: &Map<String, Value>, key: &str) -> Option<String> {
    match map.get(key)? {
        Value::String(s) if s.is_empty() => None,
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// A numeric field, from either a JSON number or a numeric string.
pub fn opt_num<T: std::str::FromStr>(map: &Map<String, Value>, key: &str) -> Option<T> {
    opt_text(map, key)?.trim().parse::<T>().ok()
}

pub fn num<T: std::str::FromStr + Default>(map: &Map<String, Value>, key: &str) -> T {
    opt_num(map, key).unwrap_or_default()
}

/// A Yahoo boolean: `1`, `"1"` or `true`.
pub fn flag(map: &Map<String, Value>, key: &str) -> bool {
    match map.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0) != 0,
        Some(Value::String(s)) => s == "1" || s.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

/// Every league in a `users;use_login=1/games;game_keys=nfl/leagues` payload.
///
/// The leagues live under each game, which lives under each user, so the
/// search is repeated per `leagues` container rather than once at the top.
pub fn user_leagues(root: &Value) -> Vec<YahooLeague> {
    let mut out = Vec::new();
    collect_league_containers(root, &mut out);
    out
}

fn collect_league_containers(value: &Value, out: &mut Vec<YahooLeague>) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                if key == "leagues" {
                    out.extend(items(inner, "league").into_iter().map(league_from));
                } else {
                    collect_league_containers(inner, out);
                }
            }
        }
        Value::Array(list) => {
            for item in list {
                collect_league_containers(item, out);
            }
        }
        _ => {}
    }
}

/// The single league in a `league/<key>` or `league/<key>/settings` payload.
pub fn league(root: &Value) -> Option<YahooLeague> {
    let found = find(root, "league")?;
    let parsed = league_from(found);
    (!parsed.league_key.is_empty()).then_some(parsed)
}

fn league_from(value: &Value) -> YahooLeague {
    let map = flatten(value);
    let mut league = YahooLeague {
        league_key: text(&map, "league_key"),
        league_id: text(&map, "league_id"),
        name: text(&map, "name"),
        season: text(&map, "season"),
        num_teams: num(&map, "num_teams"),
        draft_status: text(&map, "draft_status"),
        scoring_type: opt_text(&map, "scoring_type"),
        ..YahooLeague::default()
    };
    if let Some(settings) = map.get("settings") {
        apply_settings(&mut league, settings);
    }
    league
}

fn apply_settings(league: &mut YahooLeague, settings: &Value) {
    let map = flatten(settings);
    league.draft_time = opt_num(&map, "draft_time");
    league.draft_type = opt_text(&map, "draft_type");
    if league.scoring_type.is_none() {
        league.scoring_type = opt_text(&map, "scoring_type");
    }
    if let Some(slots) = map.get("roster_positions") {
        league.roster_positions = items(slots, "roster_position")
            .into_iter()
            .map(|slot| {
                let slot = flatten(slot);
                RosterSlot {
                    position: text(&slot, "position"),
                    count: num(&slot, "count"),
                }
            })
            .filter(|slot| !slot.position.is_empty())
            .collect();
    }
    if let Some(stats) = map.get("stat_modifiers").and_then(|s| find(s, "stats")) {
        league.stat_modifiers = items(stats, "stat")
            .into_iter()
            .filter_map(|stat| {
                let stat = flatten(stat);
                Some(StatModifier {
                    stat_id: opt_num(&stat, "stat_id")?,
                    value: opt_num(&stat, "value")?,
                })
            })
            .collect();
    }
    if let Some(stats) = map.get("stat_categories").and_then(|s| find(s, "stats")) {
        league.stat_categories = items(stats, "stat")
            .into_iter()
            .filter_map(|stat| {
                let stat = flatten(stat);
                let name = text(&stat, "name");
                Some(StatCategory {
                    stat_id: opt_num(&stat, "stat_id")?,
                    display: opt_text(&stat, "display_name").unwrap_or_else(|| name.clone()),
                    name,
                })
            })
            .collect();
    }
}

/// Every team in a `league/<key>/teams` payload.
pub fn teams(root: &Value) -> Vec<YahooTeam> {
    let Some(container) = find(root, "teams") else {
        return Vec::new();
    };
    items(container, "team")
        .into_iter()
        .map(team_from)
        .filter(|team| !team.team_key.is_empty())
        .collect()
}

fn team_from(value: &Value) -> YahooTeam {
    let map = flatten(value);
    YahooTeam {
        team_key: text(&map, "team_key"),
        team_id: text(&map, "team_id"),
        name: text(&map, "name"),
        draft_position: opt_num(&map, "draft_position"),
        managers: map
            .get("managers")
            .map(|managers| {
                items(managers, "manager")
                    .into_iter()
                    .map(|manager| {
                        let manager = flatten(manager);
                        YahooManager {
                            guid: text(&manager, "guid"),
                            nickname: text(&manager, "nickname"),
                            is_current_login: flag(&manager, "is_current_login"),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// The picks made so far in a `league/<key>/draftresults` payload.
///
/// Called during a live draft this is a partial list — Yahoo returns only the
/// picks that have happened — which is exactly what the poller diffs against.
pub fn draft_results(root: &Value) -> Vec<YahooDraftPick> {
    let Some(container) = find(root, "draftresults") else {
        return Vec::new();
    };
    items(container, "draft_result")
        .into_iter()
        .map(|pick| {
            let pick = flatten(pick);
            YahooDraftPick {
                pick: num(&pick, "pick"),
                round: num(&pick, "round"),
                team_key: text(&pick, "team_key"),
                player_key: text(&pick, "player_key"),
                cost: opt_num(&pick, "cost"),
            }
        })
        .filter(|pick| pick.pick > 0)
        .collect()
}

/// One page of `league/<key>/players` — or the players on a team's roster,
/// which Yahoo shapes identically one level deeper.
pub fn players(root: &Value) -> PlayerPage {
    let Some(container) = find(root, "players") else {
        return PlayerPage::default();
    };
    let players: Vec<YahooPlayer> = items(container, "player")
        .into_iter()
        .map(player_from)
        .filter(|player| !player.player_key.is_empty())
        .collect();
    let count = container
        .get("count")
        .and_then(|c| c.as_u64().or_else(|| c.as_str()?.parse().ok()))
        .map(|c| c as usize)
        .unwrap_or(players.len());
    PlayerPage { players, count }
}

fn player_from(value: &Value) -> YahooPlayer {
    let map = flatten(value);
    let name = map.get("name").map(flatten).unwrap_or_default();
    YahooPlayer {
        player_key: text(&map, "player_key"),
        player_id: text(&map, "player_id"),
        full_name: text(&name, "full"),
        first: text(&name, "first"),
        last: text(&name, "last"),
        editorial_team_abbr: text(&map, "editorial_team_abbr"),
        display_position: text(&map, "display_position"),
        eligible_positions: map
            .get("eligible_positions")
            .map(|positions| {
                items(positions, "position")
                    .into_iter()
                    .filter_map(|position| position.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        status: opt_text(&map, "status"),
        // `bye_weeks` is `{"week": "10"}`; a defence has none at all.
        bye_week: map
            .get("bye_weeks")
            .map(flatten)
            .and_then(|weeks| opt_num(&weeks, "week")),
        uniform_number: opt_text(&map, "uniform_number"),
    }
}

#[cfg(test)]
#[path = "yahoo_parse_tests.rs"]
mod tests;
