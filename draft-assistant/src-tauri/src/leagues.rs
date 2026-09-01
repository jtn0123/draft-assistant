//! The leagues on a Sleeper account, so switching between them is a choice
//! rather than a league ID the user has to go and find.
//!
//! Everything the picker offers comes from two places: the leagues this app
//! has already loaded (kept in the config as `StoredLeague`), and the ones
//! Sleeper says the saved account plays in this season. Both are the same
//! shape, so the UI does not have to care which list a row came from.

use crate::engine::StoredLeague;
use crate::sleeper::{League, SleeperClient, BASE};
use crate::sleeper_error::{to_message, SleeperError};
use crate::state::AppState;
use tauri::State;

/// Read-only account endpoint, declared here rather than in `sleeper.rs` for
/// the same reason the in-season ones are: next to the code that needs it.
pub trait UserLeagues {
    /// Every NFL league the account plays in that season.
    #[allow(async_fn_in_trait)]
    async fn user_leagues(&self, user_id: &str, season: &str) -> Result<Vec<League>, SleeperError>;
}

impl UserLeagues for SleeperClient {
    async fn user_leagues(&self, user_id: &str, season: &str) -> Result<Vec<League>, SleeperError> {
        let user_id = path_segment(user_id, 1..=32, "Sleeper user id")?;
        let season = path_segment(season, 4..=4, "season")?;
        let leagues: Option<Vec<League>> = self
            .get_json(&format!("{BASE}/user/{user_id}/leagues/nfl/{season}"))
            .await?;
        Ok(leagues.unwrap_or_default())
    }
}

/// Digits only, of a plausible length. Both of these are interpolated into a
/// request path, so anything else is refused rather than escaped — the same
/// rule `SleeperClient::user` applies to a username.
fn path_segment<'a>(
    value: &'a str,
    lengths: std::ops::RangeInclusive<usize>,
    what: &str,
) -> Result<&'a str, SleeperError> {
    let value = value.trim();
    if lengths.contains(&value.len()) && value.chars().all(|c| c.is_ascii_digit()) {
        return Ok(value);
    }
    Err(SleeperError::Invalid(format!("'{value}' is not a {what}")))
}

/// The leagues on the saved Sleeper account, for the league picker.
///
/// The season defaults to the one the loaded league is playing, which is the
/// only season the rest of the app has data for.
#[tauri::command]
pub async fn sleeper_leagues(
    state: State<'_, AppState>,
    season: Option<String>,
) -> Result<Vec<StoredLeague>, String> {
    let user_id = {
        let config = state.config.lock().await;
        config.my_user_id.clone()
    };
    let user_id = user_id.ok_or(
        "no Sleeper account saved — set your Sleeper username before looking up your leagues",
    )?;
    let season = match season {
        Some(season) => season,
        None => {
            let loaded = state.loaded.lock().await;
            loaded
                .as_ref()
                .ok_or("no league loaded, so there is no season to look up")?
                .league
                .season
                .clone()
        }
    };
    let leagues = state
        .engine
        .client
        .user_leagues(&user_id, &season)
        .await
        .map_err(to_message)?;
    Ok(sorted_stored(leagues))
}

/// Sleeper returns them in creation order, which means nothing to a reader.
fn sorted_stored(leagues: Vec<League>) -> Vec<StoredLeague> {
    let mut stored: Vec<StoredLeague> = leagues
        .into_iter()
        .map(|l| StoredLeague {
            league_id: l.league_id,
            name: l.name,
            season: l.season,
        })
        .collect();
    stored.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    stored
}

#[cfg(test)]
mod tests {
    use super::{path_segment, sorted_stored};
    use crate::sleeper::League;

    fn league(id: &str, name: &str) -> League {
        League {
            league_id: id.to_string(),
            name: name.to_string(),
            season: "2026".to_string(),
            status: "in_season".to_string(),
            total_rosters: 12,
            roster_positions: vec![],
            scoring_settings: Default::default(),
            draft_id: None,
            previous_league_id: None,
            settings: Default::default(),
        }
    }

    #[test]
    fn a_season_and_a_user_id_are_digits_of_the_right_length() {
        assert_eq!(path_segment("2026", 4..=4, "season").unwrap(), "2026");
        assert_eq!(
            path_segment(" 1389710366300200960 ", 1..=32, "id").unwrap(),
            "1389710366300200960"
        );
    }

    #[test]
    fn anything_that_could_walk_out_of_the_path_is_refused() {
        for junk in ["", "20xx", "../players", "2026/nfl", "20266"] {
            assert!(
                path_segment(junk, 4..=4, "season").is_err(),
                "{junk:?} should be refused"
            );
        }
    }

    #[test]
    fn leagues_come_back_in_a_readable_order() {
        let names: Vec<String> = sorted_stored(vec![
            league("3", "zeta"),
            league("1", "Alpha"),
            league("2", "middle"),
        ])
        .into_iter()
        .map(|l| l.name)
        .collect();
        assert_eq!(names, ["Alpha", "middle", "zeta"]);
    }
}
