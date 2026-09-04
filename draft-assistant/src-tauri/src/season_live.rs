//! The live NFL scoreboard, joined to the players who actually matter.
//!
//! Sleeper's scores feed is league-agnostic, so everything here is about the
//! join: which of the sixteen games contain a starter of mine or my
//! opponent's, what state that player is in, and how much of each side's
//! projection is already banked.
//!
//! Kickoff times are emitted as epoch milliseconds, never formatted strings —
//! the frontend renders them in US Eastern, which is where NFL windows are
//! named, without this crate needing a timezone database.

/// Scoreboard fixtures for the tests on both sides of the summary seam.
#[cfg(test)]
pub mod fixtures;
mod summary;

use crate::season_api::{GameMeta, ScoreGame};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// The roll-ups read off the joined games. Re-exported because callers have
/// always reached for the whole scoreboard through `season_live`.
pub use summary::{next_window, totals, windows, KickoffWindow, LiveTotals};

/// Where a player's game is: hasn't kicked off, in progress, or finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayState {
    Pre,
    Playing,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GameState {
    Pre,
    Live,
    Final,
}

impl GameState {
    fn of(meta: &GameMeta) -> Self {
        if meta.is_over {
            GameState::Final
        } else if meta.is_in_progress || meta.has_started {
            GameState::Live
        } else {
            GameState::Pre
        }
    }

    fn play_state(self) -> PlayState {
        match self {
            GameState::Pre => PlayState::Pre,
            GameState::Live => PlayState::Playing,
            GameState::Final => PlayState::Done,
        }
    }
}

/// One rostered player appearing in an NFL game.
#[derive(Debug, Clone, Serialize)]
pub struct GameChip {
    pub player_id: String,
    pub name: String,
    /// Roster slot they occupy ("RB", "FLEX", …), or their position if benched.
    pub slot: String,
    pub team: Option<String>,
    pub points: f64,
    /// True when they are on my roster, false when they are my opponent's.
    pub is_mine: bool,
    pub state: PlayState,
}

#[derive(Debug, Clone, Serialize)]
pub struct LiveGame {
    pub game_id: String,
    pub away: String,
    pub home: String,
    pub away_score: Option<i64>,
    pub home_score: Option<i64>,
    pub state: GameState,
    /// "Q3 07:12" while live, "Final" when over, empty before kickoff (the
    /// frontend renders the kickoff time from `kickoff_ms` instead).
    pub status: String,
    /// Epoch milliseconds. Groups games into kickoff windows.
    pub kickoff_ms: i64,
    /// "Red zone" when either side is inside the twenty.
    pub flag: Option<String>,
    /// Who is showing it — "CBS", "NBC/Peacock", "Netflix". None when Sleeper
    /// has not published a broadcaster for the game yet.
    pub channel: Option<String>,
    pub chips: Vec<GameChip>,
}

impl LiveGame {
    pub fn my_starter_count(&self) -> usize {
        self.chips.iter().filter(|c| c.is_mine).count()
    }
}

/// A player we care about, keyed for the join against NFL teams.
#[derive(Debug, Clone)]
pub struct TrackedPlayer {
    pub player_id: String,
    pub name: String,
    pub slot: String,
    pub team: Option<String>,
    pub points: f64,
    pub is_mine: bool,
}

fn status_text(meta: &GameMeta, state: GameState) -> String {
    match state {
        GameState::Final => "Final".to_string(),
        GameState::Live => {
            let clock = meta.time_remaining.as_deref().unwrap_or("").trim();
            match (meta.quarter_num, clock) {
                (Some(q), "") if q > 0 => format!("Q{q}"),
                (Some(q), c) if q > 0 => format!("Q{q} {c}"),
                _ => "Live".to_string(),
            }
        }
        GameState::Pre => String::new(),
    }
}

fn red_zone_flag(meta: &GameMeta, state: GameState) -> Option<String> {
    if state != GameState::Live {
        return None;
    }
    meta.red_zone
        .as_deref()
        .map(str::trim)
        .filter(|z| !z.is_empty())
        .map(|_| "Red zone".to_string())
}

/// Every NFL team code, as Sleeper spells them.
const NFL_TEAMS: [&str; 32] = [
    "ARI", "ATL", "BAL", "BUF", "CAR", "CHI", "CIN", "CLE", "DAL", "DEN", "DET", "GB", "HOU",
    "IND", "JAX", "KC", "LAC", "LAR", "LV", "MIA", "MIN", "NE", "NO", "NYG", "NYJ", "PHI", "PIT",
    "SEA", "SF", "TB", "TEN", "WAS",
];

/// Teams with no game this week, from the full NFL slate (not just the games
/// with a rostered player). Empty when no schedule has loaded, so the caller
/// can tell "no byes" from "don't know".
pub fn bye_teams(games: &[ScoreGame]) -> Vec<String> {
    if games.is_empty() {
        return Vec::new();
    }
    let playing: HashSet<&str> = games
        .iter()
        .filter_map(ScoreGame::meta)
        .flat_map(|m| [m.home_team.as_deref(), m.away_team.as_deref()])
        .flatten()
        .collect();
    NFL_TEAMS
        .iter()
        .filter(|t| !playing.contains(*t))
        .map(|t| (*t).to_string())
        .collect()
}

/// How much of a game is still to be played, as a fraction of a full one.
///
/// Only games that have kicked off are answered for: a team missing from the
/// map has not started, and callers price that player off his projection the
/// way they always have. A final game is `0.0`.
///
/// The clock is read off the quarter and the time left in it. Overtime, a
/// missing quarter or an unparseable clock all land on `HALF_LEFT` rather than
/// guessing — being roughly right about a game in progress beats being exactly
/// wrong about it.
const HALF_LEFT: f64 = 0.5;
const QUARTERS: f64 = 4.0;
const QUARTER_MINUTES: f64 = 15.0;

pub fn remaining_by_team(games: &[ScoreGame]) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    for game in games {
        let Some(meta) = game.meta() else { continue };
        let remaining = match GameState::of(meta) {
            GameState::Pre => continue,
            GameState::Final => 0.0,
            GameState::Live => live_remaining(meta),
        };
        for team in [meta.home_team.as_deref(), meta.away_team.as_deref()]
            .into_iter()
            .flatten()
        {
            let team = team.trim();
            if !team.is_empty() {
                out.insert(team.to_ascii_uppercase(), remaining);
            }
        }
    }
    out
}

/// The fraction of a live game still to come, from the quarter and the clock.
fn live_remaining(meta: &GameMeta) -> f64 {
    let Some(quarter) = meta.quarter_num.filter(|q| (1..=4).contains(q)) else {
        return HALF_LEFT;
    };
    let Some(in_quarter) = meta.time_remaining.as_deref().and_then(parse_clock) else {
        return HALF_LEFT;
    };
    let whole_quarters_left = QUARTERS - f64::from(quarter);
    ((whole_quarters_left * QUARTER_MINUTES + in_quarter) / (QUARTERS * QUARTER_MINUTES))
        .clamp(0.0, 1.0)
}

/// "07:12" -> 7.2 minutes. Anything else is not a clock.
fn parse_clock(clock: &str) -> Option<f64> {
    let (minutes, seconds) = clock.trim().split_once(':')?;
    let minutes: f64 = minutes.trim().parse().ok()?;
    let seconds: f64 = seconds.trim().parse().ok()?;
    if !(0.0..=60.0).contains(&seconds) {
        return None;
    }
    Some(minutes + seconds / 60.0)
}

/// Join the week's NFL games to the players on both sides of my matchup.
///
/// Games containing nobody from either roster are dropped: this is a fantasy
/// scoreboard, not an NFL one.
pub fn live_games(games: &[ScoreGame], tracked: &[TrackedPlayer]) -> Vec<LiveGame> {
    let mut by_team: HashMap<&str, Vec<&TrackedPlayer>> = HashMap::new();
    for player in tracked {
        if let Some(team) = player.team.as_deref() {
            by_team.entry(team).or_default().push(player);
        }
    }

    let mut out: Vec<LiveGame> = Vec::new();
    for game in games {
        let Some(meta) = game.meta() else { continue };
        let (Some(home), Some(away)) = (meta.home_team.as_deref(), meta.away_team.as_deref())
        else {
            continue;
        };
        let state = GameState::of(meta);
        let mut chips: Vec<GameChip> = Vec::new();
        for team in [away, home] {
            for player in by_team.get(team).into_iter().flatten() {
                chips.push(GameChip {
                    player_id: player.player_id.clone(),
                    name: player.name.clone(),
                    slot: player.slot.clone(),
                    team: player.team.clone(),
                    points: player.points,
                    is_mine: player.is_mine,
                    state: state.play_state(),
                });
            }
        }
        if chips.is_empty() {
            continue;
        }
        // Mine first, then highest scoring — the chips row is read left to right.
        chips.sort_by(|a, b| {
            b.is_mine
                .cmp(&a.is_mine)
                .then_with(|| b.points.total_cmp(&a.points))
        });
        out.push(LiveGame {
            game_id: game
                .game_id
                .clone()
                .unwrap_or_else(|| format!("{away}@{home}")),
            away: away.to_string(),
            home: home.to_string(),
            away_score: meta.away_score,
            home_score: meta.home_score,
            state,
            status: status_text(meta, state),
            kickoff_ms: game.start_time.unwrap_or(0),
            flag: red_zone_flag(meta, state),
            channel: meta
                .channel
                .as_deref()
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .map(str::to_string),
            chips,
        });
    }
    out.sort_by_key(|g| (g.kickoff_ms, g.away.clone()));
    out
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    #[test]
    fn bye_teams_are_the_slate_minus_everyone_playing() {
        assert!(bye_teams(&[]).is_empty());
        let mut games: Vec<ScoreGame> = Vec::new();
        let slate = [
            ("ARI", "ATL"),
            ("BAL", "BUF"),
            ("CAR", "CHI"),
            ("CIN", "CLE"),
            ("DAL", "DET"),
            ("GB", "HOU"),
            ("IND", "JAX"),
            ("KC", "LAR"),
            ("LV", "MIA"),
            ("MIN", "NE"),
            ("NO", "NYG"),
            ("NYJ", "PHI"),
            ("PIT", "SEA"),
            ("SF", "TB"),
            ("TEN", "WAS"),
        ];
        for (i, (away, home)) in slate.iter().enumerate() {
            games.push(game(&format!("g{i}"), away, home, 1000, meta_pre()));
        }
        assert_eq!(
            bye_teams(&games),
            vec!["DEN".to_string(), "LAC".to_string()]
        );
    }

    #[test]
    fn games_with_nobody_rostered_are_dropped() {
        let games = vec![
            game("a", "PIT", "BAL", 1000, meta_live(3, "07:12", "")),
            game("b", "NYG", "WAS", 1000, meta_live(1, "10:00", "")),
        ];
        let live = live_games(&games, &[player("p1", "PIT", 9.4, true)]);
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].game_id, "a");
    }

    #[test]
    fn status_reads_as_quarter_and_clock_or_final() {
        let games = vec![
            game("a", "PIT", "BAL", 1000, meta_live(3, "07:12", "")),
            game("b", "ATL", "CAR", 2000, meta_final()),
            game("c", "PHI", "DAL", 3000, meta_pre()),
        ];
        let tracked = vec![
            player("p1", "PIT", 9.4, true),
            player("p2", "ATL", 21.6, true),
            player("p3", "PHI", 22.4, true),
        ];
        let live = live_games(&games, &tracked);
        assert_eq!(live[0].status, "Q3 07:12");
        assert_eq!(live[1].status, "Final");
        assert_eq!(live[2].status, "");
        assert_eq!(live[2].state, GameState::Pre);
    }

    #[test]
    fn red_zone_flags_only_while_the_game_is_live() {
        let live = live_games(
            &[
                game("a", "PIT", "BAL", 1000, meta_live(3, "07:12", "BAL")),
                game("b", "ATL", "CAR", 2000, {
                    let mut m = meta_final();
                    m.red_zone = Some("ATL".into());
                    m
                }),
            ],
            &[
                player("p1", "PIT", 9.4, true),
                player("p2", "ATL", 1.0, true),
            ],
        );
        assert_eq!(live[0].flag.as_deref(), Some("Red zone"));
        assert_eq!(live[1].flag, None);
    }

    #[test]
    fn my_players_sort_ahead_of_the_opponents() {
        let live = live_games(
            &[game("a", "PIT", "BAL", 1000, meta_live(3, "07:12", ""))],
            &[
                player("theirs", "BAL", 30.0, false),
                player("mine", "PIT", 5.0, true),
            ],
        );
        assert!(live[0].chips[0].is_mine);
        assert_eq!(live[0].chips[0].player_id, "mine");
    }

    #[test]
    fn a_game_that_has_not_kicked_off_is_absent_rather_than_full() {
        let remaining = remaining_by_team(&[game("c", "PHI", "DAL", 3000, meta_pre())]);
        assert_eq!(remaining.get("PHI"), None);
        assert_eq!(remaining.get("DAL"), None);
    }

    #[test]
    fn a_final_game_has_nothing_left_and_both_teams_say_so() {
        let remaining = remaining_by_team(&[game("b", "ATL", "CAR", 2000, meta_final())]);
        assert_eq!(remaining.get("ATL"), Some(&0.0));
        assert_eq!(remaining.get("CAR"), Some(&0.0));
    }

    #[test]
    fn the_clock_decides_how_much_of_a_live_game_is_left() {
        // The kickoff of the third quarter: two full quarters still to come.
        let half = remaining_by_team(&[game("a", "PIT", "BAL", 1000, meta_live(3, "15:00", ""))]);
        assert!((half["PIT"] - 0.5).abs() < 1e-9);

        // The start of the second: three quarters left.
        let early = remaining_by_team(&[game("a", "PIT", "BAL", 1000, meta_live(2, "15:00", ""))]);
        assert!((early["PIT"] - 0.75).abs() < 1e-9);

        // Q4, 7:30 on the clock — an eighth of the game.
        let late = remaining_by_team(&[game("a", "PIT", "BAL", 1000, meta_live(4, "07:30", ""))]);
        assert!((late["PIT"] - 0.125).abs() < 1e-9);
    }

    #[test]
    fn a_game_whose_clock_makes_no_sense_falls_back_to_half() {
        for meta in [
            meta_live(0, "07:12", ""),
            meta_live(5, "07:12", ""),
            meta_live(2, "", ""),
            meta_live(2, "nonsense", ""),
        ] {
            let remaining = remaining_by_team(&[game("a", "PIT", "BAL", 1000, meta)]);
            assert_eq!(remaining.get("PIT"), Some(&0.5));
        }
    }
}
