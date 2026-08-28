//! The one true view: DraftView is BOTH the UI's data source and the
//! AI-readable state dump — no difference between what human and model see.

use crate::board::AvailablePlayer;
use crate::draft::{self, TeamRoster};
use crate::engine::{now_secs, AppConfig, LoadedLeague};
use crate::recommend::{recommend, Recommendation};
use crate::sleeper::Pick;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Monotonic build counter. `generated_at` is only second-resolution, which is
/// far too coarse to order a 3s poll against a click that lands in the same
/// second — so the frontend orders on this instead and drops anything older
/// than what it has already rendered.
static VIEW_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize)]
pub struct DraftStatus {
    pub draft_id: String,
    pub status: String,
    pub teams: u32,
    pub rounds: u32,
    pub pick_timer: Option<u32>,
    pub current_pick: u32,
    pub current_round: u32,
    pub on_clock_slot: u32,
    pub on_clock_name: Option<String>,
    pub my_slot: Option<u32>,
    pub is_my_pick: bool,
    pub picks_until_mine: Option<u32>,
    pub my_next_picks: Vec<u32>,
    pub total_picks_made: usize,
    pub manual_picks_active: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TierAlert {
    pub position: String,
    pub tier: u32,
    pub players_left: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecentPick {
    pub pick_no: u32,
    pub round: u32,
    pub slot: u32,
    pub slot_name: Option<String>,
    pub player_id: String,
    pub name: String,
    pub position: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DraftView {
    pub schema_version: String,
    pub generated_at: u64,
    /// Strictly increasing per build. Used by the UI to discard out-of-order
    /// updates; see [`VIEW_SEQ`].
    pub seq: u64,
    pub league: LeagueSummary,
    pub draft: DraftStatus,
    pub my_roster: Option<TeamRoster>,
    pub rosters: Vec<TeamRoster>,
    pub available: Vec<AvailablePlayer>,
    pub tier_alerts: Vec<TierAlert>,
    pub position_run: Option<String>,
    pub recommendations: Vec<Recommendation>,
    pub recent_picks: Vec<RecentPick>,
    pub replacement_baselines: HashMap<String, f64>,
    /// position -> number of league-wide startable players (incl. flex share)
    pub replacement_demand: HashMap<String, usize>,
    pub data_health: DataHealth,
}

#[derive(Debug, Clone, Serialize)]
pub struct LeagueSummary {
    pub league_id: String,
    pub name: String,
    pub season: String,
    pub total_rosters: u32,
    pub roster_positions: Vec<String>,
    pub draftable_positions: Vec<String>,
    pub scoring_settings: HashMap<String, f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataHealth {
    pub players_fetched_at: u64,
    pub projections_fetched_at: u64,
    pub weekly_fetched_at: u64,
    pub board_size: usize,
    pub warnings: Vec<String>,
    pub poll_last_success_at: Option<u64>,
    pub poll_consecutive_failures: u32,
    pub poll_last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PollHealth {
    pub last_success_at: Option<u64>,
    pub consecutive_failures: u32,
    pub last_error: Option<String>,
}

pub fn poll_health(loaded: &LoadedLeague) -> PollHealth {
    PollHealth {
        last_success_at: loaded.poll_last_success_at,
        consecutive_failures: loaded.poll_consecutive_failures,
        last_error: loaded.poll_last_error.clone(),
    }
}

fn validated_slot(slot: Option<u32>, teams: u32) -> (Option<u32>, Option<String>) {
    match slot {
        Some(value) if !(1..=teams).contains(&value) => (
            None,
            Some(format!(
                "your draft slot {value} is outside the valid range 1..={teams}"
            )),
        ),
        _ => (slot, None),
    }
}

/// Merge API picks with manual fallback picks. API picks are authoritative;
/// manual picks only fill pick numbers beyond what the API has reported.
pub fn merged_picks(api: &[Pick], manual: &[Pick]) -> Vec<Pick> {
    let mut picks = api.to_vec();
    let api_max = picks.iter().map(|p| p.pick_no).max().unwrap_or(0);
    for m in manual {
        if m.pick_no > api_max {
            picks.push(m.clone());
        }
    }
    picks.sort_by_key(|p| p.pick_no);
    picks
}

pub fn build_view(loaded: &LoadedLeague, config: &AppConfig) -> DraftView {
    let league = &loaded.league;
    let draft = &loaded.draft;
    let teams = draft.settings.teams;
    let rounds = draft.settings.rounds;

    let picks = merged_picks(&loaded.api_picks, &loaded.manual_picks);
    let total_picks = picks.len();
    let current_pick = (total_picks as u32 + 1).min(teams * rounds);
    let draft_over = total_picks as u32 >= teams * rounds;
    let current_round = (current_pick - 1) / teams + 1;
    let on_clock_slot = draft::slot_for_pick(current_pick, teams);

    // Slot display names: draft_order user ids resolved via league users.
    let mut slot_names: HashMap<u32, String> = HashMap::new();
    if let Some(order) = &draft.draft_order {
        for (user_id, slot) in order {
            let name = loaded
                .user_names
                .get(user_id)
                .cloned()
                .unwrap_or_else(|| user_id.clone());
            slot_names.insert(*slot, name);
        }
    }
    let my_slot = config.my_user_id.as_ref().and_then(|uid| {
        draft
            .draft_order
            .as_ref()
            .and_then(|order| order.get(uid).copied())
    });
    // Mock drafts (no league members loaded) may be joined under a guest id:
    // fall back to the draft creator's slot, then to the only joined human.
    // Never applies to a real league, where user_names is populated.
    let my_slot = my_slot.or_else(|| {
        if !loaded.user_names.is_empty() {
            return None;
        }
        let order = draft.draft_order.as_ref()?;
        draft
            .creators
            .as_ref()
            .and_then(|c| c.iter().find_map(|uid| order.get(uid).copied()))
            .or_else(|| {
                if order.len() == 1 {
                    order.values().next().copied()
                } else {
                    None
                }
            })
    });
    let (my_slot, slot_warning) = validated_slot(my_slot, teams);

    let my_next_picks: Vec<u32> = my_slot
        .map(|slot| {
            draft::picks_for_slot(slot, teams, rounds)
                .into_iter()
                .filter(|&p| p >= current_pick)
                .collect()
        })
        .unwrap_or_default();
    let is_my_pick = !draft_over && my_slot == Some(on_clock_slot);
    let picks_until_mine = my_next_picks.first().map(|&p| p - current_pick);
    // Survival is judged at my next pick AFTER the one I'm making now (or the
    // upcoming one if I'm not on the clock).
    let survival_pick = if is_my_pick {
        my_next_picks.get(1).copied()
    } else {
        my_next_picks.first().copied()
    };

    let taken: std::collections::HashSet<&str> =
        picks.iter().map(|p| p.player_id.as_str()).collect();

    let name_of = |player_id: &str| -> (String, String, Option<String>) {
        if let Some(&i) = loaded.board_index.get(player_id) {
            let p = &loaded.board[i];
            (p.name.clone(), p.position.clone(), p.team.clone())
        } else if let Some(meta) = loaded.player_meta.get(player_id) {
            (
                meta.full_name
                    .clone()
                    .unwrap_or_else(|| player_id.to_string()),
                meta.position.clone().unwrap_or_default(),
                meta.team.clone(),
            )
        } else {
            (player_id.to_string(), String::new(), None)
        }
    };

    let rosters = draft::build_rosters(&picks, teams, &loaded.roster_rules, &slot_names, name_of);
    let my_roster = my_slot.and_then(|slot| rosters.get((slot - 1) as usize).cloned());

    // Available players with survival probabilities.
    let available: Vec<AvailablePlayer> = loaded
        .board
        .iter()
        .filter(|p| !taken.contains(p.player_id.as_str()))
        .map(|p| AvailablePlayer {
            survival_next: survival_pick
                .and_then(|pick| p.adp.map(|adp| draft::survival_probability(adp, pick))),
            player: p.clone(),
        })
        .collect();

    // Tier alerts: top remaining tier per position and how many are left in it.
    let mut tier_alerts: Vec<TierAlert> = Vec::new();
    for pos in loaded.roster_rules.draftable_positions() {
        let mut best_tier: Option<u32> = None;
        let mut count = 0;
        for a in &available {
            if a.player.position == pos {
                match best_tier {
                    None => {
                        best_tier = Some(a.player.tier);
                        count = 1;
                    }
                    Some(t) if a.player.tier == t => count += 1,
                    _ => {}
                }
            }
        }
        if let Some(tier) = best_tier {
            tier_alerts.push(TierAlert {
                position: pos,
                tier,
                players_left: count,
            });
        }
    }

    // Position run: 4+ of the same position in the last 6 picks.
    let position_run = {
        let recent: Vec<&Pick> = picks.iter().rev().take(6).collect();
        let mut counts: HashMap<String, u32> = HashMap::new();
        for p in &recent {
            let (_, pos, _) = name_of(&p.player_id);
            if !pos.is_empty() {
                *counts.entry(pos).or_insert(0) += 1;
            }
        }
        counts
            .into_iter()
            .filter(|(_, c)| *c >= 4)
            .map(|(pos, _)| pos)
            .next()
    };

    let recommendations = recommend(
        &available,
        my_roster.as_ref(),
        &loaded.roster_rules,
        current_round,
        rounds,
        current_pick,
    );

    let recent_picks: Vec<RecentPick> = picks
        .iter()
        .rev()
        .take(10)
        .map(|p| {
            let (name, position, _) = name_of(&p.player_id);
            RecentPick {
                pick_no: p.pick_no,
                round: p.round,
                slot: p.draft_slot,
                slot_name: slot_names.get(&p.draft_slot).cloned(),
                player_id: p.player_id.clone(),
                name,
                position,
            }
        })
        .collect();

    let mut warnings = loaded.warnings.clone();
    warnings.extend(slot_warning);

    DraftView {
        schema_version: "1.2".into(),
        generated_at: now_secs(),
        seq: VIEW_SEQ.fetch_add(1, Ordering::Relaxed) + 1,
        league: LeagueSummary {
            league_id: league.league_id.clone(),
            name: league.name.clone(),
            season: league.season.clone(),
            total_rosters: league.total_rosters,
            roster_positions: league.roster_positions.clone(),
            draftable_positions: loaded.roster_rules.draftable_positions(),
            scoring_settings: league.scoring_settings.clone(),
        },
        draft: DraftStatus {
            draft_id: draft.draft_id.clone(),
            status: if draft_over {
                "complete".into()
            } else {
                draft.status.clone()
            },
            teams,
            rounds,
            pick_timer: draft.settings.pick_timer,
            current_pick,
            current_round,
            on_clock_slot,
            on_clock_name: slot_names.get(&on_clock_slot).cloned(),
            my_slot,
            is_my_pick,
            picks_until_mine,
            my_next_picks,
            total_picks_made: total_picks,
            manual_picks_active: !loaded.manual_picks.is_empty(),
        },
        my_roster,
        rosters,
        available,
        tier_alerts,
        position_run,
        recommendations,
        recent_picks,
        replacement_demand: loaded.replacement_model.demand.clone(),
        replacement_baselines: loaded.replacement_model.baseline.clone(),
        data_health: DataHealth {
            players_fetched_at: loaded.players_fetched_at,
            projections_fetched_at: loaded.projections_fetched_at,
            weekly_fetched_at: loaded.weekly_fetched_at,
            board_size: loaded.board.len(),
            warnings,
            poll_last_success_at: loaded.poll_last_success_at,
            poll_consecutive_failures: loaded.poll_consecutive_failures,
            poll_last_error: loaded.poll_last_error.clone(),
        },
    }
}

#[cfg(test)]
mod reliability_tests {
    use super::validated_slot;

    #[test]
    fn invalid_user_slots_are_rejected_before_roster_indexing() {
        assert_eq!(validated_slot(Some(0), 14).0, None);
        assert_eq!(validated_slot(Some(15), 14).0, None);
        assert_eq!(validated_slot(Some(2), 14).0, Some(2));
    }
}
