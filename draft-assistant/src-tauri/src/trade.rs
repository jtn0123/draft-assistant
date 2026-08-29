//! Trades, priced: the trade finder (every one-for-one swap with every
//! rival, kept only when both sides gain and the gain beats what the waiver
//! wire gives for free) and the offer evaluator (any offer at all, both
//! rosters before and after). Both are the lineup engine run both ways.

use crate::draft::TeamRoster;
use crate::lineup::{self, Candidate};
use crate::loaded::LoadedLeague;
use crate::roster::RosterRules;
use serde::Serialize;
use std::collections::HashMap;

/// Players per side considered for a swap: the top of each roster by
/// season points. Deep bench for deep bench moves nothing either way.
const SWAP_POOL: usize = 10;
const IDEAS_SHOWN: usize = 8;
/// A side has to gain at least this for the swap to be worth proposing.
const MIN_GAIN: f64 = 3.0;
/// Ideas per partner: the same hole filled eight ways is one idea.
const PER_PARTNER: usize = 2;
/// Free agents per position priced as the alternative to trading.
const FREE_PER_POSITION: usize = 3;
/// Their players considered as the target of a two-for-one.
const TWO_FOR_ONE_TARGETS: usize = 6;

#[derive(Debug, Clone, Serialize)]
pub struct TradeIdea {
    pub partner_slot: u32,
    pub partner_name: Option<String>,
    pub give_id: String,
    pub give: String,
    pub give_position: String,
    /// A second player going with `give` — a two-for-one, for when one is
    /// not enough to make it worth their while. They will have to drop
    /// someone to take it.
    pub also_give_id: Option<String>,
    pub also_give: Option<String>,
    pub also_give_position: Option<String>,
    pub get_id: String,
    pub get: String,
    pub get_position: String,
    /// Season points added to my lineup total, byes honoured.
    pub my_gain: f64,
    /// `my_gain` less what the best free agent at that position would add
    /// for nothing. A defense is not worth a receiver when defenses are free.
    pub over_waiver: f64,
    /// Same as `my_gain` for them — why they would say yes.
    pub their_gain: f64,
    /// Trades this manager made last season, when the league had one and
    /// they were in it. A manager who has never traded is a long shot
    /// whatever the numbers say.
    pub partner_trades: Option<u32>,
}

/// Slot -> trades last season, via the draft order's user ids.
fn trades_by_slot(loaded: &LoadedLeague) -> HashMap<u32, u32> {
    let (Some(order), Some(history)) = (loaded.draft.draft_order.as_ref(), loaded.history.as_ref())
    else {
        return HashMap::new();
    };
    order
        .iter()
        .filter_map(|(user_id, slot)| {
            let m = history.managers.iter().find(|m| &m.user_id == user_id)?;
            Some((*slot, m.trades))
        })
        .collect()
}

/// Ordering score: value first, with a point per trade the partner made
/// last season (to ten) — the same numbers land better with a dealer.
fn appeal(idea: &TradeIdea) -> f64 {
    idea.over_waiver + f64::from(idea.partner_trades.unwrap_or(0).min(10))
}

fn top_by_points(candidates: &[Candidate], n: usize) -> Vec<Candidate> {
    let mut v = candidates.to_vec();
    v.sort_by(|a, b| b.points.total_cmp(&a.points));
    v.truncate(n);
    v
}

fn swapped(roster: &[Candidate], out: &str, in_: &Candidate) -> Vec<Candidate> {
    let mut v: Vec<Candidate> = roster
        .iter()
        .filter(|c| c.player_id != out)
        .cloned()
        .collect();
    v.push(in_.clone());
    v
}

/// What the wire gives away: per position, the most the best free agents
/// would add to my lineup for nothing. Any trade has to beat that.
fn free_gain_by_position(
    my_season: &[Candidate],
    my_base: f64,
    free: &[&crate::board::BoardPlayer],
    rules: &RosterRules,
) -> HashMap<String, f64> {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let mut best: HashMap<String, f64> = HashMap::new();
    for p in free {
        let n = seen.entry(p.position.as_str()).or_insert(0);
        *n += 1;
        if *n > FREE_PER_POSITION {
            continue;
        }
        let c = Candidate {
            player_id: p.player_id.clone(),
            name: p.name.clone(),
            position: p.position.clone(),
            points: p.points,
            bye_week: p.bye_week,
            injury: p.injury_status.clone(),
        };
        let mut with = my_season.to_vec();
        with.push(c);
        let gain = lineup::season_points(&with, rules) - my_base;
        let entry = best.entry(p.position.clone()).or_insert(0.0);
        if gain > *entry {
            *entry = gain;
        }
    }
    best
}

/// One-for-one swaps with every rival that lift both sides, best for me
/// first, each measured against what the waiver wire would give for free.
pub fn ideas(
    loaded: &LoadedLeague,
    rosters: &[TeamRoster],
    my_slot: u32,
    free: &[&crate::board::BoardPlayer],
    rules: &RosterRules,
) -> Vec<TradeIdea> {
    let Some(mine) = rosters.get((my_slot - 1) as usize) else {
        return Vec::new();
    };
    let my_season = lineup::season_candidates(mine, &loaded.board, &loaded.board_index);
    let my_base = lineup::season_points(&my_season, rules);
    let my_pool = top_by_points(&my_season, SWAP_POOL);
    let free_gain = free_gain_by_position(&my_season, my_base, free, rules);
    let trades = trades_by_slot(loaded);
    let mut out: Vec<TradeIdea> = Vec::new();
    for rival in rosters.iter().filter(|r| r.slot != my_slot) {
        let their_season = lineup::season_candidates(rival, &loaded.board, &loaded.board_index);
        let their_base = lineup::season_points(&their_season, rules);
        let their_pool = top_by_points(&their_season, SWAP_POOL);
        for give in &my_pool {
            for get in &their_pool {
                let my_gain =
                    lineup::season_points(&swapped(&my_season, &give.player_id, get), rules)
                        - my_base;
                let over_waiver = my_gain - free_gain.get(&get.position).copied().unwrap_or(0.0);
                if over_waiver < MIN_GAIN {
                    continue;
                }
                let their_gain =
                    lineup::season_points(&swapped(&their_season, &get.player_id, give), rules)
                        - their_base;
                if their_gain < MIN_GAIN {
                    continue;
                }
                out.push(TradeIdea {
                    partner_slot: rival.slot,
                    partner_name: rival.display_name.clone(),
                    give_id: give.player_id.clone(),
                    give: give.name.clone(),
                    give_position: give.position.clone(),
                    also_give_id: None,
                    also_give: None,
                    also_give_position: None,
                    get_id: get.player_id.clone(),
                    get: get.name.clone(),
                    get_position: get.position.clone(),
                    my_gain,
                    over_waiver,
                    their_gain,
                    partner_trades: trades.get(&rival.slot).copied(),
                });
            }
        }
    }
    // Two-for-one: the same swaps with a second piece from my depth, kept
    // only where it is what makes the other side say yes — a one-for-one
    // that already works is not improved by throwing in a body.
    // Second pieces come from my bench: a player who starts for me is the
    // one-for-one's business, and pairing starters is what made this slow.
    let starters: std::collections::HashSet<String> = lineup::best_lineup(&my_season, rules)
        .1
        .into_iter()
        .map(|s| s.player_id)
        .collect();
    let bench: Vec<&Candidate> = my_pool
        .iter()
        .filter(|c| !starters.contains(&c.player_id))
        .collect();
    for rival in rosters.iter().filter(|r| r.slot != my_slot) {
        let their_season = lineup::season_candidates(rival, &loaded.board, &loaded.board_index);
        let their_base = lineup::season_points(&their_season, rules);
        let their_pool = top_by_points(&their_season, TWO_FOR_ONE_TARGETS);
        for give in &my_pool {
            for get in &their_pool {
                // Their side first: the cheap test, and the one that fails
                // most. A one-for-one that already works needs no sweetener.
                let one_for_one = {
                    let theirs = swapped(&their_season, &get.player_id, give);
                    lineup::season_points(&theirs, rules) - their_base
                };
                if one_for_one >= MIN_GAIN {
                    continue;
                }
                // Then mine, once per pair: losing a bench piece on top can
                // only lower this, so a pair that fails here fails for every
                // second piece.
                let mine_one = swapped(&my_season, &give.player_id, get);
                let my_gain_one = lineup::season_points(&mine_one, rules) - my_base;
                let free = free_gain.get(&get.position).copied().unwrap_or(0.0);
                if my_gain_one - free < MIN_GAIN {
                    continue;
                }
                for extra in bench.iter().filter(|e| e.player_id != give.player_id) {
                    let mut theirs_after = swapped(&their_season, &get.player_id, give);
                    theirs_after.push((*extra).clone());
                    let their_gain = lineup::season_points(&theirs_after, rules) - their_base;
                    if their_gain < MIN_GAIN {
                        continue;
                    }
                    let mut mine_after = mine_one.clone();
                    mine_after.retain(|c| c.player_id != extra.player_id);
                    let my_gain = lineup::season_points(&mine_after, rules) - my_base;
                    let over_waiver = my_gain - free;
                    if over_waiver < MIN_GAIN {
                        continue;
                    }
                    out.push(TradeIdea {
                        partner_slot: rival.slot,
                        partner_name: rival.display_name.clone(),
                        give_id: give.player_id.clone(),
                        give: give.name.clone(),
                        give_position: give.position.clone(),
                        also_give_id: Some(extra.player_id.clone()),
                        also_give: Some(extra.name.clone()),
                        also_give_position: Some(extra.position.clone()),
                        get_id: get.player_id.clone(),
                        get: get.name.clone(),
                        get_position: get.position.clone(),
                        my_gain,
                        over_waiver,
                        their_gain,
                        partner_trades: trades.get(&rival.slot).copied(),
                    });
                }
            }
        }
    }
    // Best for me first, nudged towards managers who trade; at equal value
    // the simpler deal wins.
    out.sort_by(|a, b| {
        appeal(b)
            .total_cmp(&appeal(a))
            .then(a.also_give_id.is_some().cmp(&b.also_give_id.is_some()))
    });
    // One idea per player I would get, and a couple per partner: the list
    // is for reading, not for enumerating.
    let mut per_partner: HashMap<u32, usize> = HashMap::new();
    let mut seen_get: std::collections::HashSet<String> = std::collections::HashSet::new();
    out.retain(|i| {
        if !seen_get.insert(i.get_id.clone()) {
            return false;
        }
        let n = per_partner.entry(i.partner_slot).or_insert(0);
        *n += 1;
        *n <= PER_PARTNER
    });
    out.truncate(IDEAS_SHOWN);
    out
}

/// An offer priced both ways: season totals (byes honoured) and this
/// week's lineup, before and after, for me and for them.
#[derive(Debug, Clone, Serialize)]
pub struct TradeVerdict {
    pub partner_slot: u32,
    pub partner_name: Option<String>,
    pub give: Vec<crate::lineup::Starter>,
    pub get: Vec<crate::lineup::Starter>,
    pub my_season_before: f64,
    pub my_season_after: f64,
    pub their_season_before: f64,
    pub their_season_after: f64,
    pub week: u32,
    pub my_week_before: f64,
    pub my_week_after: f64,
    pub their_week_before: f64,
    pub their_week_after: f64,
}

fn as_starters(cands: &[Candidate]) -> Vec<crate::lineup::Starter> {
    cands
        .iter()
        .map(|c| crate::lineup::Starter {
            slot: c.position.clone(),
            player_id: c.player_id.clone(),
            name: c.name.clone(),
            position: c.position.clone(),
            points: c.points,
            injury: c.injury.clone(),
        })
        .collect()
}

/// What is on the table.
#[derive(Debug, Clone)]
pub struct Offer<'a> {
    pub my_slot: u32,
    pub partner_slot: u32,
    /// Leaves my roster for theirs.
    pub give: &'a [String],
    /// Comes the other way.
    pub get: &'a [String],
    pub week: u32,
}

/// Price an offer. Every id must be on the roster it is leaving.
pub fn evaluate(
    loaded: &LoadedLeague,
    rosters: &[TeamRoster],
    offer: &Offer<'_>,
    rules: &RosterRules,
) -> Result<TradeVerdict, String> {
    let Offer {
        my_slot,
        partner_slot,
        give,
        get,
        week,
    } = *offer;
    if give.is_empty() && get.is_empty() {
        return Err("an offer needs at least one player".into());
    }
    let mine = rosters
        .get((my_slot - 1) as usize)
        .ok_or("my roster is not loaded")?;
    let theirs = rosters
        .get((partner_slot - 1) as usize)
        .filter(|_| partner_slot != my_slot)
        .ok_or_else(|| format!("no team at slot {partner_slot}"))?;
    let my_season = lineup::season_candidates(mine, &loaded.board, &loaded.board_index);
    let their_season = lineup::season_candidates(theirs, &loaded.board, &loaded.board_index);
    let pick =
        |from: &[Candidate], ids: &[String], whose: &str| -> Result<Vec<Candidate>, String> {
            ids.iter()
                .map(|id| {
                    from.iter()
                        .find(|c| &c.player_id == id)
                        .cloned()
                        .ok_or_else(|| format!("{id} is not on {whose} roster"))
                })
                .collect()
        };
    let giving = pick(&my_season, give, "my")?;
    let getting = pick(&their_season, get, "their")?;
    let after = |season: &[Candidate], out: &[String], in_: &[Candidate]| -> Vec<Candidate> {
        let mut v: Vec<Candidate> = season
            .iter()
            .filter(|c| !out.contains(&c.player_id))
            .cloned()
            .collect();
        v.extend(in_.iter().cloned());
        v
    };
    let my_after = after(&my_season, give, &getting);
    let their_after = after(&their_season, get, &giving);
    let wk = |season: &[Candidate]| {
        lineup::best_lineup(
            &lineup::week_candidates(season, &loaded.weekly_points, week),
            rules,
        )
        .0
    };
    Ok(TradeVerdict {
        partner_slot,
        partner_name: theirs.display_name.clone(),
        give: as_starters(&giving),
        get: as_starters(&getting),
        my_season_before: lineup::season_points(&my_season, rules),
        my_season_after: lineup::season_points(&my_after, rules),
        their_season_before: lineup::season_points(&their_season, rules),
        their_season_after: lineup::season_points(&their_after, rules),
        week,
        my_week_before: wk(&my_season),
        my_week_after: wk(&my_after),
        their_week_before: wk(&their_season),
        their_week_after: wk(&their_after),
    })
}

#[cfg(test)]
mod tests;
