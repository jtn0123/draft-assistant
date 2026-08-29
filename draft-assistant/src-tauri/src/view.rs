//! The one true view: DraftView is BOTH the UI's data source and the
//! AI-readable state dump — no difference between what human and model see.

use crate::board::AvailablePlayer;
use crate::draft::{self, TeamRoster};
use crate::engine::{now_secs, AppConfig, LoadedLeague};
use crate::history::LeagueHistory;
use crate::lineup::{self, ByeWeek, TeamProjection};
use crate::matchup::ThisWeek;
use crate::playoffs::TeamOdds;
use crate::recommend::{recommend, Recommendation};
use crate::results::SeasonSoFar;
use crate::sleeper::NflState;
use crate::sleeper::Pick;
use crate::trade::TradeIdea;
use crate::trades::PickOwnership;
use crate::transactions::Activity;
use crate::waivers::WaiverBoard;
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
    /// Scheduled start, ms since the epoch.
    pub start_time: Option<i64>,
    /// When the current pick's clock runs out, ms since the epoch. Only while
    /// the draft is live and has a pick timer.
    pub pick_deadline: Option<i64>,
    pub current_pick: u32,
    pub current_round: u32,
    pub on_clock_slot: u32,
    pub on_clock_name: Option<String>,
    pub my_slot: Option<u32>,
    pub is_my_pick: bool,
    pub picks_until_mine: Option<u32>,
    pub my_next_picks: Vec<u32>,
    /// A required starting slot is still empty and the picks are running
    /// out: "DEF still empty with 1 pick left". Only while drafting.
    pub starter_alert: Option<String>,
    /// Picks that do not follow the snake because they were traded:
    /// pick number -> the slot whose manager makes it. Empty in a league
    /// with no trades. The strip draws from this so it never names the
    /// wrong manager.
    pub traded_pick_slots: HashMap<u32, u32>,
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
    /// Every team's best lineup and what it projects to, best first. The
    /// draft's scoreboard: who is winning it so far.
    pub projected_standings: Vec<TeamProjection>,
    /// The week ahead: is my Sleeper lineup the best one, and who do I play.
    /// Absent without a league (mock draft) or before the schedule exists.
    pub this_week: Option<ThisWeek>,
    /// The waiver wire priced for my roster. Only once the draft is over.
    pub waivers: Option<WaiverBoard>,
    /// Record, standings, my results and projected-vs-actual, once a week
    /// of the regular season has been played.
    pub season: Option<SeasonSoFar>,
    /// The league's moves, newest first. Empty for a mock draft.
    pub activity: Vec<Activity>,
    /// One-for-one swaps that lift both my lineup and a rival's. Only once
    /// the draft is over.
    pub trade_ideas: Vec<TradeIdea>,
    /// Simulated rest of season on the league's schedule; empty without one.
    pub playoff_odds: Vec<TeamOdds>,
    /// Last season: who trades, who churns, what claims cost.
    pub history: Option<LeagueHistory>,
    /// My bye weeks, worst first. Empty without a roster.
    pub bye_weeks: Vec<ByeWeek>,
    pub replacement_baselines: HashMap<String, f64>,
    /// position -> number of league-wide startable players (incl. flex share)
    pub replacement_demand: HashMap<String, usize>,
    pub data_health: DataHealth,
}

use crate::picks::validated_slot;
pub use crate::picks::{keeper_pick_nos, merged_picks, next_open_pick, poll_fingerprint};

pub use crate::view_types::{poll_health, DataHealth, LeagueSummary, PollHealth};

pub fn build_view(loaded: &LoadedLeague, config: &AppConfig) -> DraftView {
    let league = &loaded.league;
    let draft = &loaded.draft;
    // Clamped: `teams`/`rounds` come straight off the Sleeper payload with no
    // schema guarantee, and zero would underflow `current_pick - 1` below.
    let teams = draft.settings.teams.max(1);
    let rounds = draft.settings.rounds.max(1);
    let degenerate_settings = draft.settings.teams == 0 || draft.settings.rounds == 0;

    let picks = merged_picks(&loaded.api_picks, &loaded.manual_picks);
    let total_picks = picks.len();
    let open_pick = next_open_pick(&picks, teams, rounds);
    let mut keepers = loaded.keeper_pick_nos.clone();
    keepers.extend(keeper_pick_nos(&loaded.api_picks, teams, rounds));
    let current_pick = open_pick.unwrap_or(teams * rounds);
    let draft_over = open_pick.is_none();
    let current_round = (current_pick - 1) / teams + 1;
    let (order, order_warning) = draft::DraftOrder::from_draft(draft);
    // Who *actually* picks: the snake, corrected for traded picks.
    let ownership = PickOwnership::from_draft(draft, &loaded.traded_picks, teams, rounds, order);
    let on_clock_slot = ownership.owner_slot(current_pick);

    // Slot display names: draft_order user ids resolved via league users.
    // Only real names go in here. A user id that resolves to nothing is not a
    // name: passing it on printed a 19-digit number where a manager belongs,
    // and stopped the UI's own "slot N" fallback from ever running.
    let mut slot_names: HashMap<u32, String> = HashMap::new();
    if let Some(order) = &draft.draft_order {
        for (user_id, slot) in order {
            if let Some(name) = loaded.user_names.get(user_id) {
                slot_names.insert(*slot, name.clone());
            }
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

    let made: std::collections::HashSet<u32> = picks.iter().map(|p| p.pick_no).collect();
    let my_next_picks: Vec<u32> = my_slot
        .map(|slot| {
            ownership
                .picks_owned_by(slot)
                .into_iter()
                .filter(|p| *p >= current_pick && !made.contains(p))
                .collect()
        })
        .unwrap_or_default();
    let is_my_pick = !draft_over && my_slot == Some(on_clock_slot);
    // How many picks actually have to happen before mine — keepers sitting in
    // between are already in the book and cost no time.
    let picks_until_mine = my_next_picks
        .first()
        .map(|&mine| (current_pick..mine).filter(|p| !made.contains(p)).count() as u32);
    // Survival is judged at my next pick AFTER the one I'm making now (or the
    // upcoming one if I'm not on the clock).
    let survival_pick = if is_my_pick {
        my_next_picks.get(1).copied()
    } else {
        my_next_picks.first().copied()
    };

    let taken: std::collections::HashSet<&str> =
        picks.iter().map(|p| p.player_id.as_str()).collect();

    let name_of = |player_id: &str| loaded.name_of(player_id);

    // Whose roster a pick is on: the user who made it, else (manual picks
    // carry no user) whoever owns that pick number.
    let user_slots: HashMap<&str, u32> = draft
        .draft_order
        .iter()
        .flat_map(|o| o.iter().map(|(u, s)| (u.as_str(), *s)))
        .collect();
    let slot_of = |p: &Pick| -> u32 {
        p.picked_by
            .as_deref()
            .and_then(|u| user_slots.get(u).copied())
            .unwrap_or_else(|| ownership.owner_slot(p.pick_no))
    };
    let rosters = draft::build_rosters(
        &picks,
        teams,
        &loaded.roster_rules,
        &slot_names,
        &keepers,
        slot_of,
        name_of,
    );
    let my_roster = my_slot.and_then(|slot| rosters.get((slot - 1) as usize).cloned());
    let starter_alert = crate::picks::alert_for(
        my_roster.as_ref(),
        draft.status == "drafting" && !draft_over,
        my_next_picks.len(),
    );
    let bye_weeks = lineup::bye_weeks_for(
        my_roster.as_ref(),
        &loaded.board,
        &loaded.board_index,
        &loaded.roster_rules,
    );
    let week = loaded
        .nfl_state
        .as_ref()
        .and_then(NflState::upcoming_week)
        .unwrap_or(lineup::OPENING_WEEK);
    let projected_standings = lineup::standings(
        &rosters,
        &loaded.board,
        &loaded.board_index,
        &loaded.weekly_points,
        week,
        &loaded.roster_rules,
    );
    let this_week = crate::matchup::this_week(loaded, &rosters, my_slot, week);
    let season = crate::results::season_so_far(loaded, &rosters, my_slot);
    let playoff_odds = if draft_over {
        crate::playoffs::simulate(loaded, &rosters, week)
    } else {
        Vec::new()
    };
    let activity = crate::transactions::timeline(
        &loaded.transactions,
        &crate::transactions::team_lookup(loaded, &rosters),
        &crate::transactions::name_lookup(loaded),
    );

    // Available players with survival probabilities.
    let available: Vec<AvailablePlayer> = loaded
        .board
        .iter()
        .filter(|p| !taken.contains(p.player_id.as_str()))
        .map(|p| AvailablePlayer {
            survival_next: survival_pick.and_then(|pick| {
                p.adp
                    .map(|adp| draft::survival_probability(adp, current_pick, pick))
            }),
            player: p.clone(),
        })
        .collect();
    // The free-agent pool, priced for my roster: the waiver board, and the
    // bar any trade has to clear. Only once the draft is over.
    let free: Vec<&crate::board::BoardPlayer> = available.iter().map(|a| &a.player).collect();
    let (waivers, trade_ideas) = match my_slot {
        Some(slot) if draft_over => (
            crate::waivers::board(loaded, &rosters, slot, &free, &loaded.trending),
            crate::trade::ideas(loaded, &rosters, slot, &free, &loaded.roster_rules),
        ),
        _ => (None, Vec::new()),
    };

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

    // What has actually happened, newest first. A keeper is in the book but
    // was never picked tonight — at 177 the draft has not reached it, and at
    // 139 once the draft has passed it, it is still not news — so keepers are
    // neither recent picks nor part of a positional run.
    let played: Vec<&Pick> = picks
        .iter()
        .filter(|p| p.pick_no < current_pick && !keepers.contains(&p.pick_no))
        .collect();

    // Position run: 4+ of the same position in the last 6 picks.
    let position_run = {
        let recent: Vec<&&Pick> = played.iter().rev().take(6).collect();
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

    // Nothing to recommend once every pick is in; a card would name a player
    // the user can no longer take.
    let recommendations = if draft_over {
        Vec::new()
    } else {
        recommend(
            &available,
            my_roster.as_ref(),
            &loaded.roster_rules,
            current_round,
            rounds,
            current_pick,
        )
    };

    // The clock on the current pick runs from whichever happened later: the
    // previous pick, or the draft starting.
    //
    // Not `last_picked` alone. Sleeper stamps it when *keepers* are entered
    // too — this league's keepers went in at 10:35 on draft morning, six
    // hours before the 17:00 start — so preferring it would open the draft
    // with a clock that expired before anyone sat down.
    let pick_deadline = match draft.settings.pick_timer {
        Some(timer) if timer > 0 && draft.status == "drafting" && !draft_over => {
            match (draft.last_picked, draft.start_time) {
                (Some(picked), Some(start)) => Some(picked.max(start)),
                (picked, start) => picked.or(start),
            }
            .map(|since| since + i64::from(timer) * 1000)
        }
        _ => None,
    };

    let recent_picks: Vec<RecentPick> = played
        .iter()
        .rev()
        .take(10)
        .map(|p| {
            let (name, position, _) = name_of(&p.player_id);
            let slot = slot_of(p);
            RecentPick {
                pick_no: p.pick_no,
                round: p.round,
                slot,
                slot_name: slot_names.get(&slot).cloned(),
                player_id: p.player_id.clone(),
                name,
                position,
            }
        })
        .collect();

    let mut warnings = loaded.warnings.clone();
    warnings.extend(slot_warning);
    warnings.extend(order_warning);
    if degenerate_settings {
        warnings.push(format!(
            "draft reports {} teams and {} rounds; treating both as at least 1",
            draft.settings.teams, draft.settings.rounds
        ));
    }

    DraftView {
        schema_version: "1.4".into(),
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
            start_time: draft.start_time,
            pick_deadline,
            current_pick,
            current_round,
            on_clock_slot,
            on_clock_name: slot_names.get(&on_clock_slot).cloned(),
            my_slot,
            is_my_pick,
            picks_until_mine,
            my_next_picks,
            starter_alert,
            traded_pick_slots: ownership.overrides(),
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
        projected_standings,
        this_week,
        waivers,
        season,
        activity,
        trade_ideas,
        playoff_odds,
        history: loaded.history.clone(),
        bye_weeks,
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
