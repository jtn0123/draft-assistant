//! Per-week projected points, scored under the league's own rules.
//!
//! The draft board only needs season totals, but every in-season panel is
//! week-shaped: this week's starter projections, "best lineup each week" for
//! season standings, and bye detection. This collapses the raw weekly
//! projection rows into one lookup so the season code never touches stat maps.

use crate::scoring;
use crate::sleeper::ProjectionRow;
use std::collections::{HashMap, HashSet};

/// player_id -> week -> projected points under this league's scoring.
#[derive(Debug, Clone, Default)]
pub struct WeeklyPoints {
    points: HashMap<String, HashMap<u32, f64>>,
    /// Which weeks the projection feed actually covered. A week's request can
    /// fail on its own, and a week nobody was projected for is a hole in the
    /// data rather than a league-wide bye.
    weeks: HashSet<u32>,
}

impl WeeklyPoints {
    pub fn build(weekly_rows: &[ProjectionRow], scoring_map: &HashMap<String, f64>) -> Self {
        let mut points: HashMap<String, HashMap<u32, f64>> = HashMap::new();
        let mut weeks: HashSet<u32> = HashSet::new();
        for row in weekly_rows {
            let (Some(stats), Some(week)) = (row.stats.as_ref(), row.week) else {
                continue;
            };
            // One week's bonus expectation uses that week's own stat line.
            let pts = scoring::base_points(stats, scoring_map)
                + scoring::bonus_points(&[stats], scoring_map);
            points
                .entry(row.player_id.clone())
                .or_default()
                .insert(week, pts);
            weeks.insert(week);
        }
        Self { points, weeks }
    }

    /// Projected points for one player in one week. `None` means no projection
    /// exists — a bye week, or a player Sleeper does not project.
    pub fn get(&self, player_id: &str, week: u32) -> Option<f64> {
        self.points
            .get(player_id)
            .and_then(|w| w.get(&week))
            .copied()
    }

    /// Projected points, treating a missing projection as zero. Use where a
    /// lineup slot must still be filled (a benched bye player scores nothing).
    pub fn get_or_zero(&self, player_id: &str, week: u32) -> f64 {
        self.get(player_id, week).unwrap_or(0.0)
    }

    /// True when the player has no projection this week but does have one in a
    /// neighbouring week — i.e. they are on bye rather than simply unprojected.
    ///
    /// A week the feed never delivered is not a bye for anybody. One failed
    /// week request used to put the whole league on bye at once, because every
    /// player was missing from it and every player had projections elsewhere.
    pub fn is_bye(&self, player_id: &str, week: u32) -> bool {
        if !self.weeks.contains(&week) {
            return false;
        }
        let Some(weeks) = self.points.get(player_id) else {
            return false;
        };
        !weeks.contains_key(&week) && weeks.keys().any(|w| *w != week)
    }

    /// Whether the feed carried any projection at all for this week.
    pub fn has_week(&self, week: u32) -> bool {
        self.weeks.contains(&week)
    }

    /// Mean projected points across the remaining weeks, used as the per-week
    /// scoring rate when simulating the rest of the season.
    pub fn mean_from(&self, player_id: &str, from_week: u32, to_week: u32) -> f64 {
        let Some(weeks) = self.points.get(player_id) else {
            return 0.0;
        };
        let played: Vec<f64> = (from_week..=to_week)
            .filter_map(|w| weeks.get(&w).copied())
            .collect();
        if played.is_empty() {
            return 0.0;
        }
        played.iter().sum::<f64>() / played.len() as f64
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sleeper::ProjectionRow;

    fn row(player_id: &str, week: u32, rush_yd: f64) -> ProjectionRow {
        ProjectionRow {
            player_id: player_id.into(),
            stats: Some(HashMap::from([("rush_yd".to_string(), rush_yd)])),
            player: None,
            week: Some(week),
            opponent: None,
        }
    }

    fn scoring_map() -> HashMap<String, f64> {
        HashMap::from([("rush_yd".to_string(), 0.1)])
    }

    #[test]
    fn scores_each_week_under_league_rules() {
        let wp = WeeklyPoints::build(&[row("a", 1, 100.0), row("a", 2, 50.0)], &scoring_map());
        assert!((wp.get("a", 1).unwrap() - 10.0).abs() < 1e-9);
        assert!((wp.get("a", 2).unwrap() - 5.0).abs() < 1e-9);
        assert_eq!(wp.get("a", 3), None);
    }

    #[test]
    fn a_gap_between_projected_weeks_reads_as_a_bye() {
        // Somebody else is projected in week 2, so the week itself arrived and
        // a's absence from it is a bye.
        let wp = WeeklyPoints::build(
            &[row("a", 1, 100.0), row("b", 2, 100.0), row("a", 3, 100.0)],
            &scoring_map(),
        );
        assert!(wp.is_bye("a", 2));
        // An entirely unknown player is not "on bye", just unprojected.
        assert!(!wp.is_bye("nobody", 2));
    }

    /// One week's projection request can fail on its own. Every player is then
    /// missing from that week and every player has projections either side of
    /// it, which read as a thirty-two-team bye.
    #[test]
    fn a_week_the_feed_never_delivered_puts_nobody_on_bye() {
        let wp = WeeklyPoints::build(
            &[
                row("a", 1, 100.0),
                row("b", 1, 100.0),
                row("a", 3, 100.0),
                row("b", 3, 100.0),
            ],
            &scoring_map(),
        );
        assert!(!wp.has_week(2));
        assert!(
            !wp.is_bye("a", 2),
            "a missing week is not a league-wide bye"
        );
        assert!(!wp.is_bye("b", 2));
        // The weeks that did arrive still answer for themselves.
        assert!(wp.has_week(1));
        assert!(!wp.is_bye("a", 1));
    }

    #[test]
    fn mean_ignores_bye_weeks_rather_than_averaging_in_zero() {
        let wp = WeeklyPoints::build(&[row("a", 1, 100.0), row("a", 3, 200.0)], &scoring_map());
        // (10 + 20) / 2, not (10 + 0 + 20) / 3.
        assert!((wp.mean_from("a", 1, 3) - 15.0).abs() < 1e-9);
        assert!((wp.mean_from("ghost", 1, 3)).abs() < 1e-9);
    }
}
