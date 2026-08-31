//! Read-only Sleeper endpoints used by the in-season screen.
//!
//! Same rules as `sleeper.rs`: unauthenticated GETs, every response
//! deserialized defensively so an undocumented field disappearing degrades a
//! panel rather than failing the whole load.

use crate::sleeper::{SleeperClient, BASE, BASE_UNDOC};
use crate::sleeper_error::SleeperError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Where the NFL currently is. Drives which week the season screen shows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NflState {
    #[serde(default)]
    pub week: u32,
    #[serde(default)]
    pub display_week: Option<u32>,
    #[serde(default)]
    pub season: String,
    #[serde(default)]
    pub season_type: String,
    #[serde(default)]
    pub previous_season: Option<String>,
}

impl NflState {
    /// The week to score. `display_week` leads `week` between the last game and
    /// the Tuesday rollover, which is exactly when a user is reviewing results.
    pub fn current_week(&self) -> u32 {
        self.display_week.unwrap_or(self.week).max(1)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RosterSettings {
    #[serde(default)]
    pub wins: u32,
    #[serde(default)]
    pub losses: u32,
    #[serde(default)]
    pub ties: u32,
    #[serde(default)]
    pub fpts: f64,
    #[serde(default)]
    pub fpts_decimal: f64,
    #[serde(default)]
    pub fpts_against: f64,
    #[serde(default)]
    pub fpts_against_decimal: f64,
    #[serde(default)]
    pub waiver_budget_used: f64,
    #[serde(default)]
    pub waiver_position: Option<u32>,
    #[serde(default)]
    pub total_moves: u32,
}

impl RosterSettings {
    /// Sleeper splits points into an integer part and a "decimal" part that is
    /// itself an integer (12.34 arrives as fpts=12, fpts_decimal=34).
    pub fn points_for(&self) -> f64 {
        self.fpts + self.fpts_decimal / 100.0
    }

    pub fn points_against(&self) -> f64 {
        self.fpts_against + self.fpts_against_decimal / 100.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Roster {
    pub roster_id: u32,
    #[serde(default)]
    pub owner_id: Option<String>,
    #[serde(default)]
    pub players: Option<Vec<String>>,
    #[serde(default)]
    pub starters: Option<Vec<String>>,
    #[serde(default)]
    pub reserve: Option<Vec<String>>,
    #[serde(default)]
    pub settings: RosterSettings,
}

impl Roster {
    pub fn player_ids(&self) -> &[String] {
        self.players.as_deref().unwrap_or(&[])
    }

    pub fn starter_ids(&self) -> &[String] {
        self.starters.as_deref().unwrap_or(&[])
    }
}

/// One roster's entry for one week. Both teams in a game share a `matchup_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Matchup {
    pub roster_id: u32,
    #[serde(default)]
    pub matchup_id: Option<u32>,
    #[serde(default)]
    pub points: f64,
    #[serde(default)]
    pub custom_points: Option<f64>,
    #[serde(default)]
    pub starters: Option<Vec<String>>,
    #[serde(default)]
    pub players: Option<Vec<String>>,
    #[serde(default)]
    pub players_points: Option<HashMap<String, f64>>,
}

impl Matchup {
    pub fn scored(&self) -> f64 {
        self.custom_points.unwrap_or(self.points)
    }

    pub fn points_for(&self, player_id: &str) -> Option<f64> {
        self.players_points
            .as_ref()
            .and_then(|p| p.get(player_id).copied())
    }

    pub fn starter_ids(&self) -> &[String] {
        self.starters.as_deref().unwrap_or(&[])
    }
}

/// One roster's entry in a week's matchup list.
pub fn matchup_for(matchups: &[Matchup], roster_id: u32) -> Option<&Matchup> {
    matchups.iter().find(|m| m.roster_id == roster_id)
}

/// The other side of `mine`. `None` on a bye week, where a roster has a
/// matchup entry but no `matchup_id` pairing it with anyone.
pub fn opponent_of<'a>(matchups: &'a [Matchup], mine: &Matchup) -> Option<&'a Matchup> {
    let id = mine.matchup_id?;
    matchups
        .iter()
        .find(|m| m.matchup_id == Some(id) && m.roster_id != mine.roster_id)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransactionSettings {
    #[serde(default)]
    pub waiver_bid: Option<i64>,
}

/// A waiver claim, free-agent add, or trade. `adds`/`drops` map player_id to
/// the roster_id on the receiving/losing end.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub transaction_id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub status: String,
    /// Milliseconds since epoch.
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub adds: Option<HashMap<String, u32>>,
    #[serde(default)]
    pub drops: Option<HashMap<String, u32>>,
    #[serde(default)]
    pub roster_ids: Vec<u32>,
    #[serde(default)]
    pub settings: Option<TransactionSettings>,
}

impl Transaction {
    pub fn bid(&self) -> Option<i64> {
        self.settings.as_ref().and_then(|s| s.waiver_bid)
    }
}

/// Numbers in the scores feed change JSON type with game state: `quarter_num`
/// is an integer once a game kicks off but an empty string before it, and the
/// scores are `null` pre-game. Accept any of those rather than failing the
/// whole week's scoreboard on one unplayed game.
fn flexible_number<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: std::str::FromStr + serde::de::DeserializeOwned,
{
    use serde::Deserialize as _;
    match Option::<serde_json::Value>::deserialize(deserializer)? {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(s)) => Ok(s.trim().parse::<T>().ok()),
        Some(other) => Ok(serde_json::from_value(other).ok()),
    }
}

/// Live NFL game state. Everything the scoreboard needs lives in `metadata`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GameMeta {
    #[serde(default)]
    pub home_team: Option<String>,
    #[serde(default)]
    pub away_team: Option<String>,
    #[serde(default, deserialize_with = "flexible_number")]
    pub home_score: Option<i64>,
    #[serde(default, deserialize_with = "flexible_number")]
    pub away_score: Option<i64>,
    #[serde(default, deserialize_with = "flexible_number")]
    pub quarter_num: Option<u32>,
    #[serde(default)]
    pub time_remaining: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    /// ISO-8601 kickoff, e.g. "2025-09-07T17:00:00+00:00".
    #[serde(default)]
    pub date_time: Option<String>,
    #[serde(default)]
    pub is_over: bool,
    #[serde(default)]
    pub is_in_progress: bool,
    #[serde(default)]
    pub has_started: bool,
    /// Broadcaster, as Sleeper spells it: "CBS", "NBC/Peacock", "Netflix".
    #[serde(default)]
    pub channel: Option<String>,
    /// Team abbreviation in the red zone, or "" when nobody is.
    #[serde(default)]
    pub red_zone: Option<String>,
    #[serde(default)]
    pub possession: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreGame {
    #[serde(default)]
    pub game_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    /// Milliseconds since epoch.
    #[serde(default)]
    pub start_time: Option<i64>,
    #[serde(default)]
    pub week: Option<u32>,
    #[serde(default)]
    pub metadata: Option<GameMeta>,
}

impl ScoreGame {
    pub fn meta(&self) -> Option<&GameMeta> {
        self.metadata.as_ref()
    }
}

/// One playoff bracket game. `p` is the placement decided by the game, so the
/// entry with `p == 1` names the champion in `w`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BracketMatch {
    #[serde(default)]
    pub r: Option<u32>,
    #[serde(default)]
    pub p: Option<u32>,
    /// Winning roster id, once the game has been played.
    #[serde(default)]
    pub w: Option<u32>,
    #[serde(default)]
    pub l: Option<u32>,
}

/// The in-season half of the Sleeper client.
///
/// Stated as a trait, and indexed in [`SleeperClient`]'s own doc comment, so
/// that a client method living outside `sleeper.rs` is declared rather than
/// merely found: the season endpoints sit next to the types they deserialize
/// into, and `SleeperClient` still lists everything it can do in one place.
pub trait SeasonEndpoints {
    /// Current NFL week and season. One tiny call, never cached for long.
    #[allow(async_fn_in_trait)]
    async fn nfl_state(&self) -> Result<NflState, SleeperError>;

    #[allow(async_fn_in_trait)]
    async fn rosters(&self, league_id: &str) -> Result<Vec<Roster>, SleeperError>;

    #[allow(async_fn_in_trait)]
    async fn matchups(&self, league_id: &str, week: u32) -> Result<Vec<Matchup>, SleeperError>;

    #[allow(async_fn_in_trait)]
    async fn transactions(
        &self,
        league_id: &str,
        week: u32,
    ) -> Result<Vec<Transaction>, SleeperError>;

    /// Playoff bracket. Empty until the league seeds it.
    #[allow(async_fn_in_trait)]
    async fn winners_bracket(&self, league_id: &str) -> Result<Vec<BracketMatch>, SleeperError>;

    /// Undocumented: live NFL scoreboard for one week, with quarter and clock.
    #[allow(async_fn_in_trait)]
    async fn nfl_scores(&self, season: u32, week: u32) -> Result<Vec<ScoreGame>, SleeperError>;
}

impl SeasonEndpoints for SleeperClient {
    async fn nfl_state(&self) -> Result<NflState, SleeperError> {
        self.get_json(&format!("{BASE}/state/nfl")).await
    }

    async fn rosters(&self, league_id: &str) -> Result<Vec<Roster>, SleeperError> {
        let v: Option<Vec<Roster>> = self
            .get_json(&format!("{BASE}/league/{league_id}/rosters"))
            .await?;
        Ok(v.unwrap_or_default())
    }

    async fn matchups(&self, league_id: &str, week: u32) -> Result<Vec<Matchup>, SleeperError> {
        let v: Option<Vec<Matchup>> = self
            .get_json(&format!("{BASE}/league/{league_id}/matchups/{week}"))
            .await?;
        Ok(v.unwrap_or_default())
    }

    async fn transactions(
        &self,
        league_id: &str,
        week: u32,
    ) -> Result<Vec<Transaction>, SleeperError> {
        let v: Option<Vec<Transaction>> = self
            .get_json(&format!("{BASE}/league/{league_id}/transactions/{week}"))
            .await?;
        Ok(v.unwrap_or_default())
    }

    async fn winners_bracket(&self, league_id: &str) -> Result<Vec<BracketMatch>, SleeperError> {
        let v: Option<Vec<BracketMatch>> = self
            .get_json(&format!("{BASE}/league/{league_id}/winners_bracket"))
            .await?;
        Ok(v.unwrap_or_default())
    }

    async fn nfl_scores(&self, season: u32, week: u32) -> Result<Vec<ScoreGame>, SleeperError> {
        let v: Option<Vec<ScoreGame>> = self
            .get_json(&format!("{BASE_UNDOC}/scores/nfl/regular/{season}/{week}"))
            .await?;
        Ok(v.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_decimal_points_recombine() {
        let s = RosterSettings {
            fpts: 1642.0,
            fpts_decimal: 45.0,
            fpts_against: 1500.0,
            fpts_against_decimal: 8.0,
            ..RosterSettings::default()
        };
        assert!((s.points_for() - 1642.45).abs() < 1e-9);
        assert!((s.points_against() - 1500.08).abs() < 1e-9);
    }

    #[test]
    fn display_week_leads_week_after_the_last_game() {
        let state = NflState {
            week: 3,
            display_week: Some(4),
            season: "2026".into(),
            season_type: "regular".into(),
            previous_season: Some("2025".into()),
        };
        assert_eq!(state.current_week(), 4);
    }

    #[test]
    fn missing_display_week_falls_back_and_never_returns_zero() {
        let state = NflState {
            week: 0,
            display_week: None,
            season: "2026".into(),
            season_type: "pre".into(),
            previous_season: None,
        };
        assert_eq!(state.current_week(), 1);
    }

    #[test]
    fn a_pregame_scoreboard_parses_despite_its_shifting_field_types() {
        // Exactly the shape Sleeper serves before kickoff: quarter_num is an
        // empty string, the scores are null. An int quarter must still work.
        let raw = r#"[
            {"game_id":"1","status":"pre_game","start_time":1789000000000,"week":1,
             "metadata":{"home_team":"ATL","away_team":"TB","home_score":null,
                         "away_score":null,"quarter_num":"","time_remaining":null,
                         "is_over":false,"is_in_progress":false,"has_started":false,
                         "red_zone":null}},
            {"game_id":"2","status":"complete","start_time":1789000000000,"week":1,
             "metadata":{"home_team":"BAL","away_team":"PIT","home_score":20,
                         "away_score":17,"quarter_num":3,"time_remaining":"07:12",
                         "is_over":false,"is_in_progress":true,"has_started":true,
                         "red_zone":"BAL"}}
        ]"#;
        let games: Vec<ScoreGame> = serde_json::from_str(raw).expect("pregame games must parse");
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].meta().unwrap().quarter_num, None);
        assert_eq!(games[0].meta().unwrap().home_score, None);
        assert_eq!(games[1].meta().unwrap().quarter_num, Some(3));
        assert_eq!(games[1].meta().unwrap().home_score, Some(20));
    }

    #[test]
    fn numeric_strings_are_read_as_numbers() {
        let raw = r#"{"quarter_num":"4","home_score":"21"}"#;
        let meta: GameMeta = serde_json::from_str(raw).unwrap();
        assert_eq!(meta.quarter_num, Some(4));
        assert_eq!(meta.home_score, Some(21));
    }

    #[test]
    fn custom_points_override_the_reported_total() {
        let m = Matchup {
            roster_id: 1,
            matchup_id: Some(2),
            points: 100.0,
            custom_points: Some(112.5),
            starters: None,
            players: None,
            players_points: None,
        };
        assert!((m.scored() - 112.5).abs() < 1e-9);
    }
}
