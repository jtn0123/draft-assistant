//! The advice wrapped around a start/sit call: a one-line reason a person can
//! read, the injury tag beside a name, and the moment the decision locks.
//!
//! `season_lineup` works out *what* to change from the projections alone.
//! Everything here is *why*, and *by when* — drawn from data the season screen
//! already loads: Sleeper's `injury_status` on the player dictionary, and each
//! game's `start_time` on the NFL scoreboard.

use crate::season_api::ScoreGame;
use crate::season_injury::{injury_code, injury_word, is_sidelined, PlayerFacts, OUT};
use crate::season_lineup::{Candidate, LineupCall, LineupSlot};
use crate::weekly::WeeklyPoints;
use std::collections::{HashMap, HashSet};

/// Everything a week's advice is derived from, gathered once.
pub struct WeekFacts<'a> {
    pub players: &'a dyn PlayerFacts,
    pub weekly: &'a WeeklyPoints,
    pub week: u32,
    /// This week's NFL scoreboard, which carries each game's kickoff.
    pub scores: &'a [ScoreGame],
    /// Now, in epoch milliseconds. A kickoff already past is not a deadline.
    pub now_ms: i64,
}

impl WeekFacts<'_> {
    /// The injury tag on one player, if the dictionary has one.
    pub fn injury(&self, player_id: &str) -> Option<&'static str> {
        injury_code(self.players.injury_status(player_id).as_deref())
    }

    /// One line, in plain words, for why a swap is being suggested — whatever
    /// the data actually supports, beyond the point difference itself.
    fn short_reason(&self, player_out: &str) -> String {
        if player_out.is_empty() {
            return "that starting spot is empty right now".to_string();
        }
        let name = self.players.name(player_out);
        if self.weekly.is_bye(player_out, self.week) {
            return format!("{name} is on bye this week");
        }
        if let Some(code) = self.injury(player_out) {
            return format!("{name} is listed {}", injury_word(code));
        }
        "higher projection at the same spot".to_string()
    }

    /// Calls the point maths alone would never raise: a player you have
    /// starting who is listed Out or Doubtful, paired with the best healthy
    /// body on your bench who could take the spot.
    ///
    /// Skipped when the projections already flagged that starter, and skipped
    /// when nothing on the bench can legally fill the slot — there is then no
    /// lineup move to make, and the tag beside his name in the table is the
    /// whole story.
    pub fn injury_calls(
        &self,
        current: &[LineupSlot],
        roster: &[Candidate],
        existing: &[LineupCall],
        eligible: &impl Fn(&str, &str) -> bool,
    ) -> Vec<LineupCall> {
        let starting: HashSet<&str> = current
            .iter()
            .filter_map(|s| s.player_id.as_deref())
            .collect();
        let mut spoken_for: HashSet<&str> = existing
            .iter()
            .flat_map(|c| [c.player_out_id.as_str(), c.player_in_id.as_str()])
            .collect();

        let mut calls = Vec::new();
        for slot in current {
            let Some(out_id) = slot.player_id.as_deref() else {
                continue;
            };
            let code = self.injury(out_id);
            if spoken_for.contains(out_id) || !is_sidelined(code) {
                continue;
            }
            let best = roster
                .iter()
                .filter(|c| !starting.contains(c.player_id.as_str()))
                .filter(|c| !spoken_for.contains(c.player_id.as_str()))
                .filter(|c| !is_sidelined(self.injury(&c.player_id)))
                .filter(|c| !self.weekly.is_bye(&c.player_id, self.week))
                .filter(|c| eligible(&slot.slot, &c.player_id))
                .max_by(|a, b| a.points.total_cmp(&b.points));
            let Some(best) = best else { continue };
            spoken_for.insert(out_id);
            spoken_for.insert(best.player_id.as_str());
            calls.push(self.injury_call(slot, out_id, injury_word(code.unwrap_or(OUT)), best));
        }
        calls
    }

    fn injury_call(
        &self,
        slot: &LineupSlot,
        out_id: &str,
        word: &str,
        best: &Candidate,
    ) -> LineupCall {
        let out_name = self.players.name(out_id);
        let in_name = self.players.name(&best.player_id);
        LineupCall {
            slot: slot.slot.clone(),
            player_in: in_name.clone(),
            player_in_id: best.player_id.clone(),
            player_in_team: self.players.team(&best.player_id),
            player_out: out_name.clone(),
            player_out_id: out_id.to_string(),
            gain: best.points - slot.points,
            why: format!(
                "{out_name} is listed {} this week, so your {} spot may score nothing at all. \
                 {in_name} projects {:.1} and can start there instead.",
                word.to_lowercase(),
                slot.slot,
                best.points,
            ),
            reason: Some(format!("{out_name} is listed {word} — pick a replacement")),
            locks_at_ms: None,
        }
    }

    /// Fill in the reason and the deadline on every call, then put the ones
    /// about a sidelined starter first — those are urgent however small the
    /// projected gain.
    pub fn finish(&self, calls: &mut [LineupCall]) {
        let kickoffs = kickoff_by_team(self.scores);
        for call in calls.iter_mut() {
            if call.reason.is_none() {
                call.reason = Some(self.short_reason(&call.player_out_id));
            }
            call.locks_at_ms = self.locks_at(&kickoffs, call);
        }
        calls.sort_by(|a, b| {
            let urgent = |c: &LineupCall| is_sidelined(self.injury(&c.player_out_id));
            urgent(b).cmp(&urgent(a)).then(b.gain.total_cmp(&a.gain))
        });
    }

    /// The earlier of the two players' kickoffs, ignoring any already past:
    /// once either game starts, that half of the swap can no longer be made.
    fn locks_at(&self, kickoffs: &HashMap<String, i64>, call: &LineupCall) -> Option<i64> {
        [
            call.player_in_team.clone(),
            self.players.team(&call.player_out_id),
        ]
        .into_iter()
        .flatten()
        .filter_map(|team| kickoffs.get(team.to_ascii_uppercase().as_str()).copied())
        .filter(|ms| *ms > self.now_ms)
        .min()
    }
}

/// NFL team abbreviation -> the kickoff of its game this week, in epoch
/// milliseconds. Games with no start time or no teams named are left out.
fn kickoff_by_team(scores: &[ScoreGame]) -> HashMap<String, i64> {
    let mut out = HashMap::new();
    for game in scores {
        let (Some(meta), Some(start)) = (game.meta(), game.start_time) else {
            continue;
        };
        if start <= 0 {
            continue;
        }
        for team in [meta.home_team.as_deref(), meta.away_team.as_deref()]
            .into_iter()
            .flatten()
        {
            let team = team.trim();
            if !team.is_empty() {
                out.insert(team.to_ascii_uppercase(), start);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::season_api::GameMeta;
    use crate::sleeper::ProjectionRow;

    /// A handful of players, each with a team and an optional injury status.
    struct Roster(HashMap<&'static str, (&'static str, Option<&'static str>)>);

    impl PlayerFacts for Roster {
        fn name(&self, player_id: &str) -> String {
            player_id.to_uppercase()
        }
        fn team(&self, player_id: &str) -> Option<String> {
            self.0.get(player_id).map(|(team, _)| (*team).to_string())
        }
        fn injury_status(&self, player_id: &str) -> Option<String> {
            self.0
                .get(player_id)
                .and_then(|(_, hurt)| hurt.map(str::to_string))
        }
    }

    fn roster() -> Roster {
        Roster(HashMap::from([
            ("waddle", ("MIA", Some("Out"))),
            ("shaky", ("MIA", Some("Doubtful"))),
            ("maybe", ("KC", Some("Questionable"))),
            ("healthy", ("PHI", None)),
            ("bench", ("PHI", None)),
            ("resting", ("DAL", None)),
            ("hurt_bench", ("DAL", Some("IR"))),
        ]))
    }

    /// `resting` is on bye in week 2; everyone else is projected every week.
    fn weekly() -> WeeklyPoints {
        let mut rows = Vec::new();
        for id in ["waddle", "shaky", "maybe", "healthy", "bench", "hurt_bench"] {
            for week in 1..=3 {
                rows.push(row(id, week, 100.0));
            }
        }
        rows.push(row("resting", 1, 100.0));
        rows.push(row("resting", 3, 100.0));
        WeeklyPoints::build(&rows, &HashMap::from([("rush_yd".to_string(), 0.1)]))
    }

    fn row(player_id: &str, week: u32, rush_yd: f64) -> ProjectionRow {
        ProjectionRow {
            player_id: player_id.into(),
            stats: Some(HashMap::from([("rush_yd".to_string(), rush_yd)])),
            player: None,
            week: Some(week),
            opponent: None,
        }
    }

    fn game(home: &str, away: &str, start: Option<i64>) -> ScoreGame {
        ScoreGame {
            game_id: Some(format!("{away}@{home}")),
            status: None,
            start_time: start,
            week: Some(2),
            metadata: Some(GameMeta {
                home_team: Some(home.into()),
                away_team: Some(away.into()),
                ..GameMeta::default()
            }),
        }
    }

    const SUNDAY_EARLY: i64 = 1_700_000_000_000;
    const SUNDAY_LATE: i64 = 1_700_012_000_000;

    fn scores() -> Vec<ScoreGame> {
        vec![
            game("MIA", "NYJ", Some(SUNDAY_EARLY)),
            game("PHI", "DAL", Some(SUNDAY_LATE)),
            game("KC", "LV", None),
        ]
    }

    fn facts<'a>(
        players: &'a Roster,
        weekly: &'a WeeklyPoints,
        scores: &'a [ScoreGame],
    ) -> WeekFacts<'a> {
        WeekFacts {
            players,
            weekly,
            week: 2,
            scores,
            now_ms: SUNDAY_EARLY - 3_600_000,
        }
    }

    fn slot(slot: &str, id: Option<&str>, points: f64) -> LineupSlot {
        LineupSlot {
            slot: slot.into(),
            player_id: id.map(str::to_string),
            points,
        }
    }

    fn candidate(id: &str, points: f64) -> Candidate {
        Candidate {
            player_id: id.into(),
            position: "WR".into(),
            points,
        }
    }

    fn call(player_in: &str, player_out: &str, gain: f64) -> LineupCall {
        LineupCall {
            slot: "WR".into(),
            player_in: player_in.to_uppercase(),
            player_in_id: player_in.into(),
            player_in_team: Some("PHI".into()),
            player_out: player_out.to_uppercase(),
            player_out_id: player_out.into(),
            gain,
            why: "long form".into(),
            reason: None,
            locks_at_ms: None,
        }
    }

    #[test]
    fn both_teams_in_a_game_share_its_kickoff() {
        let kickoffs = kickoff_by_team(&[
            game("PHI", "DAL", Some(SUNDAY_EARLY)),
            game("BUF", "NYJ", None),
            game("SEA", "SF", Some(0)),
        ]);
        assert_eq!(kickoffs.get("PHI"), Some(&SUNDAY_EARLY));
        assert_eq!(kickoffs.get("DAL"), Some(&SUNDAY_EARLY));
        assert_eq!(kickoffs.get("BUF"), None, "no start time is not a deadline");
        assert_eq!(kickoffs.get("SEA"), None, "nor is a zero one");
    }

    #[test]
    fn each_reason_says_the_strongest_thing_the_data_supports() {
        let (players, weekly, scores) = (roster(), weekly(), scores());
        let facts = facts(&players, &weekly, &scores);
        for (player_out, want) in [
            ("", "that starting spot is empty right now"),
            ("resting", "RESTING is on bye this week"),
            ("waddle", "WADDLE is listed Out"),
            ("shaky", "SHAKY is listed Doubtful"),
            ("maybe", "MAYBE is listed Questionable"),
            ("healthy", "higher projection at the same spot"),
        ] {
            assert_eq!(facts.short_reason(player_out), want, "for {player_out:?}");
        }
    }

    #[test]
    fn a_bye_outranks_an_injury_tag_because_it_is_certain() {
        // Sleeper leaves an injury tag on a player whose team is idle. The bye
        // is the fact that matters, so it must be the line the user reads.
        let players = Roster(HashMap::from([("resting", ("DAL", Some("Questionable")))]));
        let (weekly, scores) = (weekly(), scores());
        let facts = facts(&players, &weekly, &scores);
        assert_eq!(facts.short_reason("resting"), "RESTING is on bye this week");
    }

    #[test]
    fn a_sidelined_starter_becomes_a_call_the_maths_would_not_raise() {
        let (players, weekly, scores) = (roster(), weekly(), scores());
        let facts = facts(&players, &weekly, &scores);
        let current = vec![
            slot("WR", Some("waddle"), 14.0),
            slot("WR", Some("maybe"), 9.0),
        ];
        let roster_players = vec![
            candidate("waddle", 14.0),
            candidate("maybe", 9.0),
            // The best bench body is hurt himself, so the healthy one wins.
            candidate("hurt_bench", 20.0),
            candidate("bench", 11.0),
            candidate("resting", 30.0),
        ];
        let calls = facts.injury_calls(&current, &roster_players, &[], &|_, _| true);

        assert_eq!(calls.len(), 1, "only Out and Doubtful raise a call");
        let call = &calls[0];
        assert_eq!(call.player_out_id, "waddle");
        assert_eq!(
            call.player_in_id, "bench",
            "a hurt or idle bench is no help"
        );
        assert_eq!(
            call.reason.as_deref(),
            Some("WADDLE is listed Out — pick a replacement")
        );
        assert!(call.why.contains("is listed out this week"));
        assert!((call.gain - -3.0).abs() < 1e-9, "an honest, negative gain");
    }

    #[test]
    fn a_starter_the_projections_already_flagged_is_not_called_twice() {
        let (players, weekly, scores) = (roster(), weekly(), scores());
        let facts = facts(&players, &weekly, &scores);
        let current = vec![slot("WR", Some("waddle"), 14.0)];
        let roster_players = vec![candidate("waddle", 14.0), candidate("bench", 11.0)];
        let existing = vec![call("bench", "waddle", 2.0)];
        assert!(facts
            .injury_calls(&current, &roster_players, &existing, &|_, _| true)
            .is_empty());
    }

    #[test]
    fn no_eligible_bench_body_means_no_call_to_make() {
        let (players, weekly, scores) = (roster(), weekly(), scores());
        let facts = facts(&players, &weekly, &scores);
        let current = vec![slot("WR", Some("waddle"), 14.0)];
        let roster_players = vec![candidate("waddle", 14.0), candidate("bench", 11.0)];
        assert!(facts
            .injury_calls(&current, &roster_players, &[], &|_, _| false)
            .is_empty());
    }

    #[test]
    fn finishing_adds_the_reason_and_the_deadline_and_leads_with_the_urgent_one() {
        let (players, weekly, scores) = (roster(), weekly(), scores());
        let facts = facts(&players, &weekly, &scores);
        // A fat projection gain, and a small one about a player who is Out.
        let mut calls = vec![call("bench", "healthy", 8.0), call("bench", "waddle", 0.5)];
        facts.finish(&mut calls);

        assert_eq!(calls[0].player_out_id, "waddle", "the injury leads");
        assert_eq!(calls[0].reason.as_deref(), Some("WADDLE is listed Out"));
        // Waddle plays in Miami's early game; the incoming bench player is in
        // the later one, so the swap must be made before the earlier kickoff.
        assert_eq!(calls[0].locks_at_ms, Some(SUNDAY_EARLY));
        assert_eq!(
            calls[1].reason.as_deref(),
            Some("higher projection at the same spot")
        );
        assert_eq!(calls[1].locks_at_ms, Some(SUNDAY_LATE));
    }

    #[test]
    fn a_reason_already_written_is_left_alone() {
        let (players, weekly, scores) = (roster(), weekly(), scores());
        let facts = facts(&players, &weekly, &scores);
        let mut calls = vec![call("bench", "waddle", 0.5)];
        calls[0].reason = Some("already said".into());
        facts.finish(&mut calls);
        assert_eq!(calls[0].reason.as_deref(), Some("already said"));
    }

    #[test]
    fn a_kickoff_already_past_is_not_offered_as_a_deadline() {
        let (players, weekly, scores) = (roster(), weekly(), scores());
        let mut facts = facts(&players, &weekly, &scores);
        facts.now_ms = SUNDAY_EARLY + 1;
        let mut calls = vec![call("bench", "waddle", 0.5)];
        facts.finish(&mut calls);
        // Miami has kicked off, so only Philadelphia's later game is a deadline.
        assert_eq!(calls[0].locks_at_ms, Some(SUNDAY_LATE));

        facts.now_ms = SUNDAY_LATE + 1;
        facts.finish(&mut calls);
        assert_eq!(calls[0].locks_at_ms, None, "nothing left to decide");
    }

    #[test]
    fn a_player_whose_game_is_not_on_the_scoreboard_gets_no_deadline() {
        let (players, weekly) = (roster(), weekly());
        let facts = facts(&players, &weekly, &[]);
        let mut calls = vec![call("bench", "waddle", 0.5)];
        facts.finish(&mut calls);
        assert_eq!(calls[0].locks_at_ms, None);
    }
}
