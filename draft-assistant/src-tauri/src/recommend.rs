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

/// What the best player at `position` is worth by the time you pick again.
///
/// Each candidate is the best survivor only if every better one at that
/// position is gone, so the expectation walks down the board multiplying by
/// the chance each is taken. Independence is an approximation — a run makes
/// picks correlate — but it is the honest shape: deep positions come out
/// near their current best (waiting costs nothing), thin ones fall away.
fn expected_best_at_next_pick(available: &[AvailablePlayer], position: &str) -> f64 {
    let mut ranked: Vec<&AvailablePlayer> = available
        .iter()
        .filter(|a| a.player.position == position)
        .collect();
    ranked.sort_by(|a, b| b.player.vorp.total_cmp(&a.player.vorp));
    let mut expected = 0.0;
    let mut all_gone = 1.0;
    // Past a dozen deep the remaining probability mass is negligible and the
    // survival model is guesswork anyway.
    for a in ranked.iter().take(12) {
        let survives = a.survival_next.unwrap_or(0.9).clamp(0.0, 1.0);
        expected += all_gone * survives * a.player.vorp;
        all_gone *= 1.0 - survives;
        if all_gone < 0.01 {
            break;
        }
    }
    expected
}

pub fn recommend(
    available: &[AvailablePlayer],
    my_roster: Option<&TeamRoster>,
    rules: &RosterRules,
    current_round: u32,
    total_rounds: u32,
    current_pick: u32,
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
            // What you lose by waiting, not what the player is worth in the
            // abstract. Taking a receiver from a band four deep gains you
            // almost nothing over your next pick; taking the last tier-4 tight
            // end gains you the whole cliff behind him. Absolute VORP stays in
            // as a small tiebreak so two equal drop-offs settle on the better
            // player. Before this the two were 0.6/point against a flat six
            // points for "he will not last", so scarcity could never win.
            let expected_later = expected_best_at_next_pick(available, &p.position);
            // Both ends floored at replacement level. Below it a player is
            // worth no more than the waiver wire, so the gap between the best
            // of a bad lot and the expected best of that same bad lot is not a
            // cliff — it is noise. Unfloored, a barren position generated the
            // biggest drop-off on the board and this recommended a running
            // back at minus two VORP over an eighteen-VORP receiver.
            let dropoff = (p.vorp.max(0.0) - expected_later.max(0.0)).max(0.0);
            let value = dropoff + p.vorp * 0.12;
            let mut score = value;
            reasons.push(format!("{:.0} VORP under league scoring", p.vorp));
            if dropoff >= 8.0 {
                reasons.push(format!(
                    "{dropoff:.0} VORP better than what {} is likely to offer at your next pick",
                    p.position
                ));
            } else if dropoff <= 2.0 {
                reasons.push(format!(
                    "{} is deep — similar value should still be there next time",
                    p.position
                ));
            }

            // How many of this position I already hold. Every need term
            // below is relative to it: the same slot is worth much less to
            // the sixth man at a position than to the first.
            let count = have.get(p.position.as_str()).copied().unwrap_or(0);
            // Crowding, 1 down to 1/7: your own players at a position compete
            // with each other for the flex slots they would fill.
            let crowding = 1.0 / (1.0 + f64::from(count));

            // Roster need: dedicated slot open, then flex eligibility.
            let open_slot = rules.first_open_slot_for(&open, &p.position);
            if open_slot == Some(p.position.as_str()) {
                // A dedicated slot names the position, so nothing else can
                // fill it and crowding does not apply.
                score += 12.0 * need_pressure.min(2.0);
                reasons.push(format!("fills open {} starter slot", p.position));
            } else if let Some(slot) = open_slot {
                // A flex slot does not care which position fills it, and
                // crediting every position the same for it is what built a
                // roster of five receivers and two backs: once RB, WR and TE
                // were each covered once, four flex slots said yes to
                // everyone and the receivers won every tiebreak on raw value.
                score += 8.0 * need_pressure.min(2.0) * crowding;
                reasons.push(format!("fills an open {slot} slot"));
            } else {
                // Starters filled is the normal state for the whole back half
                // of a draft, so a flat penalty on everyone was a no-op that
                // only flattened the field and let raw value pick every time.
                // Depth is worth less the more of it you already hold: a sixth
                // receiver is behind five of your own for the same four flex
                // slots, while a third back is all that stands between one
                // injury and a waiver pickup in your starting lineup.
                //
                // A discount on the gain, not a tax on the position. A flat
                // penalty big enough to matter is a positional ban, and a
                // receiver far enough ahead should still win — which is the
                // line the user drew: backs and tight ends first, unless the
                // gap in value is large.
                let crowding = 1.0 / (1.0 + 0.35 * f64::from(count));
                score = value.max(0.0) * crowding + value.min(0.0) - 4.0;
                reasons.push(if count >= 4 {
                    format!("depth behind {count} other {}s you hold", p.position)
                } else {
                    "depth pick — starters already filled at position".into()
                });
            }

            // Two of one offence at one position share a quarterback, a target
            // share and a bye week: when one booms the other usually busts,
            // and both are out the same Sunday.
            if let (Some(team), Some(roster)) = (p.team.as_deref(), my_roster) {
                let same = roster
                    .players
                    .iter()
                    .filter(|x| x.position == p.position && x.team.as_deref() == Some(team))
                    .count();
                if same > 0 {
                    score -= 10.0;
                    reasons.push(format!("you already roster a {team} {}", p.position));
                }
            }

            // Positional discipline (fantasy-bot's documented failure modes,
            // fixed): backups at onesie positions are near-worthless, and a
            // second DEF is worthless outright.
            match p.position.as_str() {
                "DEF" | "K" => {
                    if count >= 1 {
                        continue; // never draft a second defense or kicker
                    }
                    // Last two rounds, not three: a defense is worth about
                    // what the next one is worth, and the round spent on it a
                    // round earlier is a round not spent on a flier.
                    if current_round < total_rounds.saturating_sub(1) {
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
                    // Only a backup if nothing would start him. A superflex
                    // slot makes a second QB a starter, and the penalty used
                    // to fire anyway — hidden until the drop-off model shrank
                    // the raw-value term that had been paying for it.
                    if count == 1 && open_slot.is_none() {
                        score -= 25.0;
                        reasons.push("backup QB — only at extreme value".into());
                    }
                }
                "TE" => {
                    if count >= 2 {
                        continue; // a third TE is a wasted roster spot
                    }
                    if count == 1 && open_slot.is_none() {
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
                    // Only for a player actually worth rostering. "Thin at
                    // RB" is a reason to take a good back, never a reason to
                    // take a bad one: below replacement he is worse than what
                    // the waiver wire gives away for free, and this bonus was
                    // large enough to carry a minus-24 back to the top of the
                    // board on its own.
                    if count < 3 && p.vorp > 0.0 {
                        score += 3.0 * (3 - count) as f64;
                        reasons.push(format!("thin at {} ({count} rostered)", p.position));
                    }
                    // No flat "already N rostered" penalty here any more: the
                    // crowding discount above is that penalty, expressed as a
                    // discount on the gain rather than a tax on the position.
                    // Charging both put a seventh receiver 12 points in the
                    // hole and had this recommending a back at minus 24 VORP
                    // over a receiver at plus 13.
                }
            }

            // Scarcity: last-of-tier boost.
            let tier_left = available
                .iter()
                .filter(|x| x.player.position == p.position && x.player.tier == p.tier)
                .count();
            // Halved when the drop-off model landed: "last of his tier" is a
            // proxy for scarcity, and scarcity is now measured directly above.
            // At the old +8 it could outvote the thing it was standing in for.
            // Same gate: a tier among players nobody should roster is not
            // scarcity, it is an artefact of where the tier lines fell.
            if tier_left <= 2 && p.vorp > 0.0 {
                score += 4.0;
                reasons.push(format!(
                    "only {tier_left} left in {} tier {}",
                    p.position, p.tier
                ));
            }

            // No flat bonus here any more: the drop-off above already prices
            // the chance of losing him. This only says the odds out loud.
            if let Some(surv) = a.survival_next {
                if surv > 0.7 {
                    reasons.push(format!(
                        "{:.0}% likely to survive to your next pick",
                        surv * 100.0
                    ));
                } else if surv < 0.35 {
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

            if mode == "safe" {
                // Safe mode: penalize real injury flags and volatile
                // bonus-heavy value.
                if let Some(status) = &p.injury_status {
                    if serious_injury(status) {
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

/// Statuses that mean a player may actually miss games. Through August Sleeper
/// tags a large share of healthy starters `Questionable` (rest days, minor
/// preseason knocks) — on draft night that flag alone is noise, not a signal.
pub fn serious_injury(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "out" | "ir" | "pup" | "sus" | "doubtful" | "na" | "cov"
    )
}
