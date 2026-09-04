//! How far a lineup's real score wanders from the projection it was built on.
//!
//! Everything downstream that talks in probabilities — this week's win odds,
//! the playoff simulation — needs a spread, not just a mean. A flat "team
//! scores land near 27% of their projection" was the old answer, and it is
//! wrong in both directions: a lineup fronted by a quarterback is steadier
//! than that, one leaning on a defense and a kicker is wilder, and two
//! starters who share an NFL team rise and fall together rather than
//! independently.
//!
//! ## Provenance
//!
//! The per-position spreads are measured, not guessed: every starter
//! projected four points or more across a full league season — 1,746
//! player-weeks — root mean square of (actual - projected) / projected.
//!
//! [`SPREAD_CALIBRATION`] is fitted out of sample: 1.3 on the first half of a
//! season cut the log loss of the second half from 0.626 to 0.605, so the
//! improvement is not the fit flattering itself. It sits a notch under the
//! 1.5 the whole season asked for, on 98 games of evidence.

use crate::season_lineup::LineupSlot;

/// Week-to-week spread of one player around his projection, as a fraction of
/// it. Wide, because a weekly projection is a mean over boom and bust games:
/// a quarterback's week is the steadiest, a defense's the wildest.
pub fn position_cv(position: &str) -> f64 {
    match position {
        "QB" => 0.44,
        "RB" => 0.57,
        "WR" => 0.63,
        "TE" => 0.62,
        "K" => 0.6,
        "DEF" | "DST" => 0.77,
        _ => 0.6,
    }
}

/// The spread a season of real games wanted, over the one the starters' own
/// spreads add up to. Above 1 because a projection can be wrong about the
/// week in ways a scoring distribution does not cover — a benched back, a
/// game script, an injury in the first quarter — and those upsets land in the
/// tails, where the normal is thin.
const SPREAD_CALIBRATION: f64 = 1.3;

/// Two starters on the same NFL team rise and fall together — a quarterback
/// and his receiver most of all. Applied between same-team starters on one
/// side of a matchup.
const STACK_CORRELATION: f64 = 0.3;

/// The spread of a team whose starters are not known, as a fraction of its
/// projected total. Roughly what the per-starter model above produces for an
/// ordinary nine-slot lineup, so a team missing its lineup detail is not
/// quietly given a different distribution from everyone else.
pub const FALLBACK_TEAM_CV: f64 = 0.27;

/// One starter, reduced to what a spread needs to know about him.
#[derive(Debug, Clone)]
pub struct Starter {
    pub position: String,
    /// NFL team, for the stack correlation. `None` never stacks.
    pub team: Option<String>,
    /// What he is expected to finish the week on: banked points plus whatever
    /// of his projection is still to be played.
    pub points: f64,
    /// The part of `points` that is not yet settled. Equal to `points` before
    /// kickoff, zero once his game is final — a player who has finished can no
    /// longer move the score, so he contributes nothing to the spread.
    pub uncertain: f64,
}

/// What a player's game has done to his week so far.
#[derive(Debug, Clone, Copy)]
pub struct LiveScore {
    /// Fantasy points already scored.
    pub banked: f64,
    /// How much of his game is still to be played, 0.0..=1.0.
    pub remaining: f64,
}

/// Resolve a filled lineup into the starters a spread is computed over.
/// Empty slots contribute nothing and are dropped.
pub fn starters_of(
    lineup: &[LineupSlot],
    position_of: &impl Fn(&str) -> Option<String>,
    team_of: &impl Fn(&str) -> Option<String>,
) -> Vec<Starter> {
    lineup
        .iter()
        .filter_map(|slot| {
            let id = slot.player_id.as_deref()?;
            Some(Starter {
                position: position_of(id).unwrap_or_default(),
                team: team_of(id),
                points: slot.points,
                uncertain: slot.points,
            })
        })
        .collect()
}

/// The same resolution, but with each starter priced off where his game
/// actually is.
///
/// A projection is the right number for a player who has not kicked off. It is
/// the wrong one all Sunday afternoon: a starter who has finished on 4.2 is
/// worth 4.2 and cannot move again, and one at half time is worth what he has
/// plus half of what he was projected for. Pricing the week off projections
/// alone is why the win odds used to read the same at midnight Sunday as they
/// did at kickoff.
///
/// `live_of` returns `None` for a player whose game has not started, which
/// reproduces [`starters_of`] exactly.
pub fn live_starters(
    lineup: &[LineupSlot],
    position_of: &impl Fn(&str) -> Option<String>,
    team_of: &impl Fn(&str) -> Option<String>,
    live_of: &impl Fn(&str) -> Option<LiveScore>,
) -> Vec<Starter> {
    lineup
        .iter()
        .filter_map(|slot| {
            let id = slot.player_id.as_deref()?;
            let projected = slot.points;
            let (points, uncertain) = match live_of(id) {
                Some(live) => {
                    let remaining = live.remaining.clamp(0.0, 1.0);
                    let left = projected * remaining;
                    (live.banked + left, left)
                }
                None => (projected, projected),
            };
            Some(Starter {
                position: position_of(id).unwrap_or_default(),
                team: team_of(id),
                points,
                uncertain,
            })
        })
        .collect()
}

/// A side's variance: each starter's own, plus the covariance of the pairs
/// who share an NFL team.
fn team_variance(starters: &[Starter]) -> f64 {
    let sigmas: Vec<f64> = starters
        .iter()
        .map(|s| position_cv(&s.position) * s.uncertain)
        .collect();
    let mut variance: f64 = sigmas.iter().map(|x| x * x).sum();
    for i in 0..starters.len() {
        for j in (i + 1)..starters.len() {
            let same = match (starters[i].team.as_deref(), starters[j].team.as_deref()) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            };
            if same {
                variance += 2.0 * STACK_CORRELATION * sigmas[i] * sigmas[j];
            }
        }
    }
    variance
}

/// A side's calibrated standard deviation for one week.
pub fn team_sigma(starters: &[Starter]) -> f64 {
    SPREAD_CALIBRATION * team_variance(starters).sqrt()
}

/// The spread to use for a team whose starters were never resolved.
pub fn fallback_sigma(mean: f64) -> f64 {
    mean.abs() * FALLBACK_TEAM_CV
}

/// The total points a side is projected for.
pub fn total_points(starters: &[Starter]) -> f64 {
    starters.iter().map(|s| s.points).sum::<f64>() + 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn starter(position: &str, team: Option<&str>, points: f64) -> Starter {
        Starter {
            position: position.into(),
            team: team.map(str::to_string),
            points,
            uncertain: points,
        }
    }

    fn lineup() -> Vec<LineupSlot> {
        vec![
            LineupSlot {
                slot: "QB".into(),
                player_id: Some("qb".into()),
                points: 20.0,
            },
            LineupSlot {
                slot: "WR".into(),
                player_id: Some("wr".into()),
                points: 12.0,
            },
        ]
    }

    fn position_of(id: &str) -> Option<String> {
        Some(id.to_uppercase())
    }

    fn team_of(_: &str) -> Option<String> {
        Some("BUF".to_string())
    }

    /// Nobody has kicked off, so the live resolution must reproduce the
    /// projection-only one exactly — mean and spread both.
    #[test]
    fn before_kickoff_the_live_reading_is_the_projected_one() {
        let lineup = lineup();
        let plain = starters_of(&lineup, &position_of, &team_of);
        let live = live_starters(&lineup, &position_of, &team_of, &|_| None);
        assert_eq!(live.len(), plain.len());
        for (a, b) in live.iter().zip(&plain) {
            assert!((a.points - b.points).abs() < 1e-12);
            assert!((a.uncertain - b.uncertain).abs() < 1e-12);
            assert_eq!(a.position, b.position);
        }
        assert!((team_sigma(&live) - team_sigma(&plain)).abs() < 1e-12);
        assert!((total_points(&live) - total_points(&plain)).abs() < 1e-12);
    }

    #[test]
    fn a_finished_starter_is_worth_what_he_scored_and_can_move_no_further() {
        let live = live_starters(&lineup(), &position_of, &team_of, &|id| {
            Some(LiveScore {
                banked: if id == "qb" { 4.2 } else { 30.0 },
                remaining: 0.0,
            })
        });
        assert!((total_points(&live) - 34.2).abs() < 1e-9);
        assert_eq!(team_sigma(&live), 0.0, "a settled week has no spread left");
    }

    #[test]
    fn a_starter_at_half_time_keeps_half_his_projection_and_half_his_spread() {
        let live = live_starters(&lineup(), &position_of, &team_of, &|id| {
            (id == "qb").then_some(LiveScore {
                banked: 9.0,
                remaining: 0.5,
            })
        });
        // 9 banked + half of 20 projected, and the receiver untouched.
        assert!((live[0].points - 19.0).abs() < 1e-9);
        assert!((live[0].uncertain - 10.0).abs() < 1e-9);
        assert!((live[1].points - 12.0).abs() < 1e-9);
        assert!((live[1].uncertain - 12.0).abs() < 1e-9);
    }

    #[test]
    fn a_quarterback_is_steadier_than_a_defense() {
        assert!(position_cv("QB") < position_cv("WR"));
        assert!(position_cv("WR") < position_cv("DEF"));
        assert_eq!(position_cv("DST"), position_cv("DEF"));
        // Anything the dictionary spells some other way lands on the middle.
        assert_eq!(position_cv("FB"), 0.6);
    }

    #[test]
    fn the_same_points_spread_wider_at_a_wilder_position() {
        let steady = team_sigma(&[starter("QB", None, 20.0)]);
        let wild = team_sigma(&[starter("DEF", None, 20.0)]);
        assert!(wild > steady, "{wild} vs {steady}");
        // Exactly the calibrated multiple of the position's own spread.
        assert!((steady - SPREAD_CALIBRATION * 0.44 * 20.0).abs() < 1e-9);
    }

    #[test]
    fn a_stack_is_riskier_than_the_same_two_players_on_different_teams() {
        let stacked = [
            starter("QB", Some("BUF"), 20.0),
            starter("WR", Some("BUF"), 15.0),
        ];
        let apart = [
            starter("QB", Some("BUF"), 20.0),
            starter("WR", Some("KC"), 15.0),
        ];
        let extra = 2.0 * STACK_CORRELATION * (0.44 * 20.0) * (0.63 * 15.0);
        assert!((team_variance(&stacked) - team_variance(&apart) - extra).abs() < 1e-9);
        // A missing team never stacks, not even with another missing one.
        let unknown = [starter("QB", None, 20.0), starter("WR", None, 15.0)];
        assert!((team_variance(&unknown) - team_variance(&apart)).abs() < 1e-9);
    }

    #[test]
    fn a_whole_lineup_spreads_far_less_than_any_one_starter() {
        // Nine independent starters diversify: the side's spread is a much
        // smaller fraction of its total than a single player's is of his.
        let lineup: Vec<Starter> = (0..9).map(|_| starter("WR", None, 12.0)).collect();
        let total = total_points(&lineup);
        assert!((team_sigma(&lineup) / total) < 0.3);
        // And it lands near the fallback the unresolved case uses.
        assert!((team_sigma(&lineup) - fallback_sigma(total)).abs() < 0.15 * total);
    }

    #[test]
    fn empty_slots_are_dropped_and_the_rest_resolved() {
        let lineup = vec![
            LineupSlot {
                slot: "QB".into(),
                player_id: Some("qb".into()),
                points: 20.0,
            },
            LineupSlot {
                slot: "WR".into(),
                player_id: None,
                points: 0.0,
            },
        ];
        let position_of = |id: &str| Some(id.to_uppercase());
        let team_of = |_: &str| Some("BUF".to_string());
        let starters = starters_of(&lineup, &position_of, &team_of);
        assert_eq!(starters.len(), 1);
        assert_eq!(starters[0].position, "QB");
        assert_eq!(starters[0].team.as_deref(), Some("BUF"));
        assert_eq!(total_points(&starters), 20.0);
    }
}
