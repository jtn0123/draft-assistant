//! What the week's advice adds on top of the point maths: the reason line, the
//! deadline, and the calls that are no longer worth making. Its own file only
//! because `season_calls.rs` is at the line cap.

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

/// The bug: a swap whose players are already on the field stayed at the
/// top of the screen, offering a change Sleeper would refuse.
#[test]
fn a_call_whose_game_has_kicked_off_is_dropped_rather_than_carried() {
    let (players, weekly, scores) = (roster(), weekly(), scores());
    let mut facts = facts(&players, &weekly, &scores);

    // Waddle is in Miami's early game; the bench body is in the later one.
    // Before either kicks off the call stands, with the earlier deadline.
    let mut calls = vec![call("bench", "waddle", 0.5)];
    facts.finish(&mut calls);
    assert_eq!(calls[0].locks_at_ms, Some(SUNDAY_EARLY));

    // Miami kicks off: the player coming out is playing, so there is no
    // longer a swap to make.
    facts.now_ms = SUNDAY_EARLY + 1;
    let mut calls = vec![call("bench", "waddle", 0.5)];
    facts.finish(&mut calls);
    assert!(calls.is_empty(), "a started starter cannot be benched");

    // And the other half counts too: both players in Philadelphia's later
    // game, which has now started.
    facts.now_ms = SUNDAY_LATE + 1;
    let mut calls = vec![call("bench", "healthy", 8.0)];
    facts.finish(&mut calls);
    assert!(calls.is_empty(), "a started replacement cannot be started");
}

#[test]
fn a_call_survives_when_neither_game_is_on_the_scoreboard() {
    // Kansas City has no start time, so nothing says "maybe" has begun.
    let (players, weekly, scores) = (roster(), weekly(), scores());
    let mut facts = facts(&players, &weekly, &scores);
    facts.now_ms = SUNDAY_LATE + 1;
    let mut only_kc = call("maybe", "maybe", 1.0);
    only_kc.player_in_team = Some("KC".into());
    let mut calls = vec![only_kc];
    facts.finish(&mut calls);
    assert_eq!(calls.len(), 1, "an unknown kickoff is not a kickoff");
    assert_eq!(calls[0].locks_at_ms, None);
}

#[test]
fn a_player_whose_game_is_not_on_the_scoreboard_gets_no_deadline() {
    let (players, weekly) = (roster(), weekly());
    let facts = facts(&players, &weekly, &[]);
    let mut calls = vec![call("bench", "waddle", 0.5)];
    facts.finish(&mut calls);
    assert_eq!(calls[0].locks_at_ms, None);
}
