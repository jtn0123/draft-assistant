//! Deterministic, auditable pick recommendations (safe + balanced modes).

use crate::board::AvailablePlayer;
use crate::draft::TeamRoster;
use crate::roster::RosterRules;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct Recommendation {
    pub mode: String,
    pub player_id: String,
    pub name: String,
    pub position: String,
    pub team: Option<String>,
    pub points: f64,
    pub vorp: f64,
    pub tier: u32,
    pub adp: Option<f64>,
    pub survival_next: Option<f64>,
    pub score: f64,
    pub reasons: Vec<String>,
}

pub fn recommend(
    available: &[AvailablePlayer],
    my_roster: Option<&TeamRoster>,
    rules: &RosterRules,
    current_round: u32,
    total_rounds: u32,
    current_pick: u32,
    // League size, so the imported second opinion can say how many rounds
    // late the market is rather than how many picks.
    teams: u32,
) -> Vec<Recommendation> {
    let open: HashMap<String, u32> = my_roster
        .map(|r| r.open_starters.iter().cloned().collect())
        .unwrap_or_else(|| {
            rules
                .slots()
                .iter()
                .filter(|slot| !RosterRules::is_non_starting(slot))
                .fold(HashMap::new(), |mut m, s| {
                    *m.entry(s.clone()).or_insert(0) += 1;
                    m
                })
        });
    let rounds_left = total_rounds.saturating_sub(current_round) + 1;
    let total_open: u32 = open.values().sum();
    // When open starting slots ~= rounds left, filling starters is urgent.
    let need_pressure = total_open as f64 / rounds_left.max(1) as f64;

    // How many of each position I already roster — backup discipline depends
    // on it (one DEF ever, one QB/TE unless value, diminishing flex depth).
    let mut have: HashMap<&str, u32> = HashMap::new();
    if let Some(roster) = my_roster {
        for p in &roster.players {
            *have.entry(p.position.as_str()).or_insert(0) += 1;
        }
    }

    let mut recs: Vec<Recommendation> = Vec::new();
    // Candidates: top of the overall board PLUS the top few at every position.
    // Overall-only would bury e.g. late-round RBs (negative VORP) under a
    // wall of WRs and never even consider them.
    let mut candidates: Vec<&AvailablePlayer> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut per_pos: HashMap<&str, u32> = HashMap::new();
    for (i, a) in available.iter().enumerate() {
        let pos_count = per_pos.entry(a.player.position.as_str()).or_insert(0);
        let take = i < 60 || *pos_count < 10;
        if take && seen.insert(a.player.player_id.as_str()) {
            *pos_count += 1;
            candidates.push(a);
        }
    }
    if candidates.is_empty() {
        return recs;
    }

    for mode in ["balanced", "safe"] {
        let mut best: Option<(f64, &AvailablePlayer, Vec<String>)> = None;
        for a in &candidates {
            let p = &a.player;
            let mut reasons: Vec<String> = Vec::new();
            // Fixed scale: 0.6 pts of score per VORP point. Normalizing by the
            // board-best VORP explodes late in drafts when that value is small.
            let mut score = p.vorp * 0.6;
            reasons.push(format!("{:.0} VORP under league scoring", p.vorp));

            // Roster need: dedicated slot open, then flex eligibility.
            let open_slot = rules.first_open_slot_for(&open, &p.position);
            if open_slot == Some(p.position.as_str()) {
                score += 12.0 * need_pressure.min(2.0);
                reasons.push(format!("fills open {} starter slot", p.position));
            } else if let Some(slot) = open_slot {
                score += 8.0 * need_pressure.min(2.0);
                reasons.push(format!("fills an open {slot} slot"));
            } else {
                score -= 10.0;
                reasons.push("depth pick — starters already filled at position".into());
            }

            // Positional discipline (fantasy-bot's documented failure modes,
            // fixed): backups at onesie positions are near-worthless, and a
            // second DEF is worthless outright.
            let count = have.get(p.position.as_str()).copied().unwrap_or(0);
            match p.position.as_str() {
                "DEF" | "K" => {
                    if count >= 1 {
                        continue; // never draft a second defense or kicker
                    }
                    if current_round < total_rounds.saturating_sub(2) {
                        score -= 60.0; // never early either
                    } else {
                        score += 15.0;
                        reasons.push(format!("last rounds — lock in your one {}", p.position));
                    }
                }
                "QB" => {
                    if count >= 2 {
                        continue;
                    }
                    if count == 1 {
                        score -= 25.0;
                        reasons.push("backup QB — only at extreme value".into());
                    }
                }
                "TE" => {
                    if count >= 2 {
                        continue; // a third TE is a wasted roster spot
                    }
                    if count == 1 {
                        score -= 20.0;
                        reasons.push("backup TE — only at real value".into());
                    }
                }
                _ => {
                    // RB/WR: reward filling the thin side of the flex pool,
                    // and dampen piling past 5 of a kind. Past mid-draft, a
                    // position with <2 bodies is one injury from an empty
                    // starting slot — escalate hard.
                    if count < 2 && current_round > 8 {
                        score += 20.0;
                        reasons.push(format!(
                            "only {count} {} rostered — one injury from an empty slot",
                            p.position
                        ));
                    }
                    if count < 3 {
                        score += 3.0 * (3 - count) as f64;
                        reasons.push(format!("thin at {} ({count} rostered)", p.position));
                    } else if count > 5 {
                        score -= 6.0 * (count - 5) as f64;
                        reasons.push(format!("already {count} {}s rostered", p.position));
                    }
                }
            }

            // Scarcity: last-of-tier boost.
            let tier_left = available
                .iter()
                .filter(|x| x.player.position == p.position && x.player.tier == p.tier)
                .count();
            if tier_left <= 2 {
                score += 8.0;
                reasons.push(format!(
                    "only {tier_left} left in {} tier {}",
                    p.position, p.tier
                ));
            }

            // Survival: if they'll likely make it back to my next pick, waiting
            // is an option — that lowers urgency now.
            if let Some(surv) = a.survival_next {
                if surv > 0.7 {
                    score -= 6.0;
                    reasons.push(format!(
                        "{:.0}% likely to survive to your next pick",
                        surv * 100.0
                    ));
                } else if surv < 0.35 {
                    score += 6.0;
                    reasons.push(format!(
                        "only {:.0}% chance they last to your next pick",
                        surv * 100.0
                    ));
                }
            }

            // Market signal, classic definition: still available past their
            // ADP = falling value; drafting far ahead of ADP = a reach the
            // market says can probably wait.
            if let Some(adp) = p.adp {
                let past_adp = current_pick as f64 - adp;
                if past_adp > 8.0 {
                    score += 5.0;
                    reasons.push(format!("falling: {past_adp:.0} picks past ADP {adp:.0}"));
                } else if past_adp < -25.0 {
                    score -= 3.0;
                    reasons.push(format!("ahead of market (ADP {adp:.0})"));
                }
            }

            // An imported second opinion, but only when it runs the user's
            // way: this player is further up someone else's board than he is
            // up this one. Built next to the rest of the reasons, in
            // `second_opinion.rs`.
            if let Some(reason) = crate::second_opinion::rec_reason(p, teams) {
                score += 4.0;
                reasons.push(reason);
            }

            if mode == "safe" {
                // Safe mode: penalize injury flags and volatile bonus-heavy value.
                if let Some(status) = &p.injury_status {
                    if !status.is_empty() {
                        score -= 15.0;
                        reasons.push(format!("injury flag: {status}"));
                    }
                }
                score -= (p.bonus_points / p.points.max(1.0)) * 40.0;
                if let Some(adp) = p.adp {
                    // Stay close to market in safe mode.
                    let reach = (p.overall_rank as f64 - adp).max(0.0);
                    score -= reach * 0.3;
                }
            }

            if best.as_ref().map(|(s, _, _)| score > *s).unwrap_or(true) {
                best = Some((score, a, reasons));
            }
        }
        // Safety net: if every candidate was disqualified (deep-roster edge
        // cases), fall back to best available so a pick is always suggested.
        if best.is_none() {
            if let Some(a) = candidates.first() {
                best = Some((0.0, a, vec!["best available (fallback)".into()]));
            }
        }
        if let Some((score, a, reasons)) = best {
            recs.push(Recommendation {
                mode: mode.into(),
                player_id: a.player.player_id.clone(),
                name: a.player.name.clone(),
                position: a.player.position.clone(),
                team: a.player.team.clone(),
                points: a.player.points,
                vorp: a.player.vorp,
                tier: a.player.tier,
                adp: a.player.adp,
                survival_next: a.survival_next,
                score,
                reasons,
            });
        }
    }
    recs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::AvailablePlayer;
    use crate::board::BoardPlayer;
    use crate::draft::RosterEntry;

    fn player(id: &str, pos: &str, vorp: f64) -> AvailablePlayer {
        AvailablePlayer {
            player: BoardPlayer {
                player_id: id.into(),
                name: id.into(),
                position: pos.into(),
                team: None,
                bye_week: None,
                points: 150.0 + vorp,
                bonus_points: 0.0,
                vorp,
                tier: 1,
                position_rank: 1,
                overall_rank: 1,
                adp: Some(100.0),
                injury_status: None,
                sleeper_pts_ppr: None,
                second_opinion: None,
            },
            survival_next: None,
        }
    }

    fn entry(pos: &str, n: u32) -> RosterEntry {
        RosterEntry {
            player_id: format!("{pos}{n}"),
            name: format!("{pos}{n}"),
            position: pos.into(),
            team: None,
            pick_no: n,
            round: n,
            is_keeper: false,
        }
    }

    fn roster(positions: &[&str]) -> TeamRoster {
        TeamRoster {
            slot: 2,
            display_name: None,
            players: positions
                .iter()
                .enumerate()
                .map(|(i, p)| entry(p, i as u32 + 1))
                .collect(),
            open_starters: vec![("FLEX".into(), 2)],
        }
    }

    fn slots() -> Vec<String> {
        [
            "QB", "RB", "WR", "TE", "FLEX", "FLEX", "FLEX", "FLEX", "DEF", "BN",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    #[test]
    fn never_recommends_second_def() {
        // A monster-VORP second DEF must lose to a modest RB.
        let available = vec![player("def2", "DEF", 90.0), player("rb1", "RB", 30.0)];
        let mine = roster(&["QB", "RB", "WR", "TE", "DEF"]);
        let recs = recommend(
            &available,
            Some(&mine),
            &RosterRules::new(&slots()),
            14,
            15,
            180,
            12,
        );
        assert!(recs.iter().all(|r| r.position != "DEF"), "{recs:?}");
    }

    #[test]
    fn never_recommends_third_qb() {
        let available = vec![player("qb3", "QB", 95.0), player("wr1", "WR", 20.0)];
        let mine = roster(&["QB", "QB", "RB", "WR"]);
        let recs = recommend(
            &available,
            Some(&mine),
            &RosterRules::new(&slots()),
            10,
            15,
            130,
            12,
        );
        assert!(recs.iter().all(|r| r.position != "QB"), "{recs:?}");
    }

    #[test]
    fn locks_def_in_final_rounds_when_missing() {
        let available = vec![player("def1", "DEF", 40.0), player("wr9", "WR", 42.0)];
        let mut mine = roster(&["QB", "RB", "RB", "WR", "WR", "WR", "TE"]);
        // As the engine would report it: the DEF starter slot is still open.
        mine.open_starters = vec![("DEF".into(), 1), ("FLEX".into(), 1)];
        let recs = recommend(
            &available,
            Some(&mine),
            &RosterRules::new(&slots()),
            14,
            15,
            184,
            12,
        );
        assert_eq!(recs[0].position, "DEF", "{recs:?}");
    }

    #[test]
    fn fallback_when_all_disqualified() {
        // Only a second DEF available — fallback must still recommend it.
        let available = vec![player("def2", "DEF", 50.0)];
        let mine = roster(&["QB", "RB", "WR", "TE", "DEF"]);
        let recs = recommend(
            &available,
            Some(&mine),
            &RosterRules::new(&slots()),
            15,
            15,
            200,
            12,
        );
        assert!(!recs.is_empty());
    }

    #[test]
    fn superflex_qb_is_recognized_as_filling_a_starter() {
        let available = vec![player("qb2", "QB", 80.0), player("wr2", "WR", 10.0)];
        let mut mine = roster(&["QB", "RB", "WR", "TE"]);
        mine.open_starters = vec![("SUPER_FLEX".into(), 1)];
        let superflex_slots = ["QB", "RB", "WR", "TE", "SUPER_FLEX", "BN"]
            .iter()
            .map(|slot| (*slot).to_string())
            .collect::<Vec<_>>();

        let recs = recommend(
            &available,
            Some(&mine),
            &RosterRules::new(&superflex_slots),
            5,
            10,
            50,
            12,
        );

        assert_eq!(recs[0].player_id, "qb2", "{recs:?}");
        assert!(recs[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("SUPER_FLEX")));
    }

    #[test]
    fn never_recommends_a_second_kicker() {
        let available = vec![player("k2", "K", 100.0), player("rb2", "RB", 10.0)];
        let mine = roster(&["QB", "RB", "WR", "TE", "K"]);
        let kicker_slots = ["QB", "RB", "WR", "TE", "K", "BN"]
            .iter()
            .map(|slot| (*slot).to_string())
            .collect::<Vec<_>>();
        let recs = recommend(
            &available,
            Some(&mine),
            &RosterRules::new(&kicker_slots),
            9,
            10,
            90,
            12,
        );
        assert!(recs
            .iter()
            .all(|recommendation| recommendation.position != "K"));
    }
}
