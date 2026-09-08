//! Account names for the current Sleeper league's identity picker.
use crate::sleeper::LeagueUser;
use crate::state::AppState;
use serde::Serialize;
use std::collections::HashMap;
use tauri::State;

#[derive(Debug, Serialize)]
pub struct SleeperMember {
    user_id: String,
    display_name: Option<String>,
    draft_slot: Option<u32>,
    is_current: bool,
}

fn members(
    users: Vec<LeagueUser>,
    order: &HashMap<String, u32>,
    current: Option<&str>,
) -> Vec<SleeperMember> {
    let mut result: Vec<_> = users
        .into_iter()
        .map(|user| SleeperMember {
            draft_slot: order.get(&user.user_id).copied(),
            is_current: current == Some(user.user_id.as_str()),
            user_id: user.user_id,
            // Use the account name, never the custom fantasy-team label.
            display_name: user.display_name.filter(|name| !name.trim().is_empty()),
        })
        .collect();
    result.sort_by_cached_key(|user| {
        (
            user.display_name.clone().unwrap_or_default().to_lowercase(),
            user.user_id.clone(),
        )
    });
    result.dedup_by(|a, b| a.user_id == b.user_id);
    result
}

#[tauri::command]
pub async fn list_sleeper_members(
    state: State<'_, AppState>,
) -> Result<Vec<SleeperMember>, String> {
    crate::applog::logged!(
        "list_sleeper_members",
        String::new(),
        list_sleeper_members_inner(&state).await
    )
}

async fn list_sleeper_members_inner(state: &AppState) -> Result<Vec<SleeperMember>, String> {
    let (league_id, order) = {
        let loaded = state.loaded.lock().await;
        let loaded = loaded.as_ref().ok_or("no league loaded")?;
        (
            loaded.league.league_id.clone(),
            loaded.draft.draft_order.clone().unwrap_or_default(),
        )
    };
    if crate::view_types::is_yahoo_key(&league_id) {
        return Err("Choose your account through Yahoo sign-in".into());
    }
    let users = state
        .engine
        .client
        .league_users_quick(&league_id)
        .await
        .map_err(crate::sleeper_error::to_message)?;
    // A league switch during the lookup must not show another league's users.
    if state
        .loaded
        .lock()
        .await
        .as_ref()
        .map(|loaded| &loaded.league.league_id)
        != Some(&league_id)
    {
        return Err("The league changed. Reload its account list.".into());
    }
    let current = state.config.lock().await.my_user_id.clone();
    Ok(members(users, &order, current.as_deref()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accounts_remain_selectable_before_order_and_use_account_not_team_names() {
        let users = serde_json::from_value(serde_json::json!([
            {"user_id":"2", "display_name":"zoe", "metadata":{"team_name":"Champions"}},
            {"user_id":"1", "display_name":"alex"}
        ]))
        .unwrap();
        let result = members(users, &HashMap::new(), Some("2"));
        assert_eq!(result[0].display_name.as_deref(), Some("alex"));
        assert_eq!(result[1].display_name.as_deref(), Some("zoe"));
        assert!(result[1].is_current);
        assert!(result.iter().all(|user| user.draft_slot.is_none()));
    }
    #[test]
    fn draft_order_never_adds_accounts_outside_the_league() {
        let users = serde_json::from_value(serde_json::json!([
            {"user_id":"1", "display_name":"alex"}
        ]))
        .unwrap();
        let order = HashMap::from([("1".into(), 3), ("outsider".into(), 1)]);
        let result = members(users, &order, None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].draft_slot, Some(3));
    }
}
