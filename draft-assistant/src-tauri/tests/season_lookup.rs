//! Player identity resolution, which every season section goes through.
//!
//! Two dictionaries back it: the draft board (rich, but only players who were
//! draftable) and Sleeper's player metadata (thin, but complete). The
//! interesting half is what happens when neither is complete — an id nobody
//! has heard of, a metadata row with no position, a defence that exists only
//! in the dictionary — because that is what a season screen actually meets
//! once free agency starts.

mod common;

use draft_assistant_lib::board::BoardPlayer;
use draft_assistant_lib::engine::LoadedLeague;
use draft_assistant_lib::season_injury::PlayerFacts;
use draft_assistant_lib::season_lookup::Lookup;
use draft_assistant_lib::sleeper::PlayerMeta;

/// A metadata row with everything absent, to be filled in per test. Written
/// out rather than defaulted because `PlayerMeta` mirrors Sleeper's payload
/// and deliberately has no `Default`.
fn meta() -> PlayerMeta {
    PlayerMeta {
        full_name: None,
        first_name: None,
        last_name: None,
        position: None,
        team: None,
        fantasy_positions: None,
        injury_status: None,
        years_exp: None,
        age: None,
    }
}

fn some(value: &str) -> Option<String> {
    Some(value.to_string())
}

/// The fixture league, plus whatever metadata rows a test needs.
fn league_with(metadata: &[(&str, PlayerMeta)]) -> LoadedLeague {
    let (mut loaded, _, _) = common::fixture();
    for (id, row) in metadata {
        std::sync::Arc::make_mut(&mut loaded.player_meta).insert((*id).to_string(), row.clone());
    }
    loaded
}

/// The board row for `player_id`, to be edited in place.
fn board_row<'a>(loaded: &'a mut LoadedLeague, player_id: &str) -> &'a mut BoardPlayer {
    let i = loaded.board_index[player_id];
    // Shared behind an `Arc` so the poll tick can copy the loaded league
    // without duplicating it; editing one takes a copy of its own.
    &mut std::sync::Arc::make_mut(&mut loaded.board)[i]
}

#[test]
fn an_id_in_neither_dictionary_degrades_to_the_id_itself() {
    let loaded = league_with(&[]);
    let lookup = Lookup { loaded: &loaded };

    // A newly signed practice-squad player can appear in a roster before he
    // appears in the cached player dictionary. He must still render.
    assert_eq!(lookup.position("9999"), None);
    assert_eq!(lookup.name("9999"), "9999");
    assert_eq!(lookup.team("9999"), None);
    assert_eq!(lookup.injury("9999"), None);
}

#[test]
fn a_player_on_the_board_is_read_off_it() {
    let loaded = league_with(&[]);
    let lookup = Lookup { loaded: &loaded };

    assert_eq!(lookup.name("q1"), "Ace Passer");
    assert_eq!(lookup.position("q1").as_deref(), Some("QB"));
    assert_eq!(lookup.team("q1").as_deref(), Some("ATL"));
    assert_eq!(lookup.injury("q1"), None);
}

#[test]
fn a_defence_lives_only_in_the_metadata_and_still_resolves() {
    let loaded = league_with(&[(
        "SF",
        PlayerMeta {
            full_name: some("San Francisco 49ers"),
            position: some("DEF"),
            team: some("SF"),
            ..meta()
        },
    )]);
    let lookup = Lookup { loaded: &loaded };

    assert_eq!(lookup.name("SF"), "San Francisco 49ers");
    assert_eq!(lookup.position("SF").as_deref(), Some("DEF"));
    assert_eq!(lookup.team("SF").as_deref(), Some("SF"));
}

#[test]
fn a_metadata_row_with_no_position_or_team_reports_neither() {
    let loaded = league_with(&[
        (
            "no-position",
            PlayerMeta {
                full_name: some("Positionless Person"),
                ..meta()
            },
        ),
        (
            // Sleeper writes a blank string rather than omitting the key on
            // some rows; that is not a position either.
            "blank-position",
            PlayerMeta {
                full_name: some("Blank Person"),
                position: some(""),
                team: some("FA"),
                ..meta()
            },
        ),
    ]);
    let lookup = Lookup { loaded: &loaded };

    assert_eq!(lookup.position("no-position"), None);
    assert_eq!(lookup.team("no-position"), None, "no team key, no team");
    assert_eq!(
        lookup.position("blank-position"),
        None,
        "an empty string is not a position"
    );
    assert_eq!(lookup.team("blank-position").as_deref(), Some("FA"));
}

#[test]
fn a_name_is_assembled_from_the_parts_when_there_is_no_full_name() {
    let loaded = league_with(&[
        (
            "split-name",
            PlayerMeta {
                first_name: some("Given"),
                last_name: some("Family"),
                ..meta()
            },
        ),
        (
            "half-name",
            PlayerMeta {
                first_name: some("Given"),
                ..meta()
            },
        ),
        ("nameless", meta()),
    ]);
    let lookup = Lookup { loaded: &loaded };

    assert_eq!(lookup.name("split-name"), "Given Family");
    assert_eq!(
        lookup.name("half-name"),
        "half-name",
        "half a name is not a name; fall back to the id"
    );
    assert_eq!(lookup.name("nameless"), "nameless");
}

#[test]
fn injury_status_is_reported_only_when_there_is_one() {
    let loaded = league_with(&[
        (
            "healthy",
            PlayerMeta {
                full_name: some("Healthy Player"),
                ..meta()
            },
        ),
        (
            "hurt",
            PlayerMeta {
                full_name: some("Hurt Player"),
                injury_status: some("Questionable"),
                ..meta()
            },
        ),
        (
            // Cleared players keep the key with whitespace in it rather than
            // losing it, which used to render an empty injury pill.
            "cleared",
            PlayerMeta {
                full_name: some("Cleared Player"),
                injury_status: some("   "),
                ..meta()
            },
        ),
    ]);
    let lookup = Lookup { loaded: &loaded };

    assert_eq!(lookup.injury("healthy"), None);
    assert_eq!(lookup.injury("hurt").as_deref(), Some("Questionable"));
    assert_eq!(
        lookup.injury("cleared"),
        None,
        "a blank status is no status"
    );
}

#[test]
fn the_board_wins_over_the_metadata_for_a_player_in_both() {
    // The board is rebuilt from this league's own scoring and roster rules,
    // so where the two disagree the board is the newer answer. A recent fix
    // depends on this ordering.
    let mut loaded = league_with(&[(
        "q1",
        PlayerMeta {
            full_name: some("Stale Name"),
            position: some("WR"),
            team: some("STALE"),
            injury_status: some("Out"),
            ..meta()
        },
    )]);
    {
        let row = board_row(&mut loaded, "q1");
        row.injury_status = Some("Questionable".to_string());
    }
    let lookup = Lookup { loaded: &loaded };

    assert_eq!(lookup.name("q1"), "Ace Passer");
    assert_eq!(lookup.position("q1").as_deref(), Some("QB"));
    assert_eq!(lookup.team("q1").as_deref(), Some("ATL"));
    assert_eq!(
        lookup.injury("q1").as_deref(),
        Some("Questionable"),
        "the board's status must not be overwritten by a stale dictionary"
    );
}

#[test]
fn the_board_answering_with_nothing_is_still_the_board_answering() {
    // A free agent on the board has no NFL team, and a board row that says
    // "no injury" is a real answer rather than a reason to consult the
    // dictionary. Both are how the board wins even when it says nothing.
    let mut loaded = league_with(&[(
        "q1",
        PlayerMeta {
            team: some("STALE"),
            injury_status: some("Out"),
            ..meta()
        },
    )]);
    {
        let row = board_row(&mut loaded, "q1");
        row.team = None;
        row.injury_status = None;
    }
    let lookup = Lookup { loaded: &loaded };

    assert_eq!(lookup.team("q1"), None, "the board says he has no team");
    assert_eq!(
        lookup.injury("q1").as_deref(),
        Some("Out"),
        "the board carries no status, so the dictionary is consulted"
    );
}

#[test]
fn a_blank_status_on_the_board_is_not_a_reason_to_ask_the_dictionary() {
    // The board explicitly cleared him. Falling through to a dictionary that
    // has not caught up would put the pill back.
    let mut loaded = league_with(&[(
        "q1",
        PlayerMeta {
            injury_status: some("Out"),
            ..meta()
        },
    )]);
    board_row(&mut loaded, "q1").injury_status = Some(String::new());
    let lookup = Lookup { loaded: &loaded };

    assert_eq!(lookup.injury("q1"), None);
}

#[test]
fn the_player_facts_view_is_the_same_answer() {
    // Injury and lineup code takes `Lookup` through this trait rather than
    // concretely, so the two spellings must not drift apart.
    let loaded = league_with(&[(
        "SF",
        PlayerMeta {
            full_name: some("San Francisco 49ers"),
            team: some("SF"),
            injury_status: some("Out"),
            ..meta()
        },
    )]);
    let lookup = Lookup { loaded: &loaded };
    let facts: &dyn PlayerFacts = &lookup;

    assert_eq!(facts.name("SF"), "San Francisco 49ers");
    assert_eq!(facts.team("SF").as_deref(), Some("SF"));
    assert_eq!(facts.injury_status("SF").as_deref(), Some("Out"));
    assert_eq!(facts.name("q1"), "Ace Passer");
    assert_eq!(facts.injury_status("q1"), None);
    assert_eq!(facts.name("9999"), "9999", "and the unknown id survives it");
}
