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
    pub points: f64,
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
            })
        })
        .collect()
}

/// A side's variance: each starter's own, plus the covariance of the pairs
/// who share an NFL team.
fn team_variance(starters: &[Starter]) -> f64 {
    let sigmas: Vec<f64> = starters
        .iter()
        .map(|s| position_cv(&s.position) * s.points)
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
        }
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
