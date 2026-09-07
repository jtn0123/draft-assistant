use super::{nonce, sorted_stored, yahoo_leagues_inner};
use crate::state::{AppState, YahooState};
use crate::yahoo::YahooHosts;
use crate::yahoo_types::YahooLeague;
use std::sync::Arc;

fn league(key: &str, name: &str, status: &str) -> YahooLeague {
    YahooLeague {
        league_key: key.to_string(),
        league_id: key.rsplit('.').next().unwrap_or(key).to_string(),
        name: name.to_string(),
        season: "2026".to_string(),
        num_teams: 12,
        draft_status: status.to_string(),
        ..YahooLeague::default()
    }
}

#[test]
fn the_picker_gets_yahoo_leagues_in_a_readable_order() {
    let stored = sorted_stored(vec![
        league("449.l.3", "zeta", "predraft"),
        league("449.l.1", "Alpha", "draft"),
        league("449.l.2", "middle", "postdraft"),
    ]);
    let names: Vec<&str> = stored.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Alpha", "middle", "zeta"]);
}

#[test]
fn every_row_says_it_is_a_yahoo_league_and_where_its_draft_has_got_to() {
    let stored = sorted_stored(vec![
        league("449.l.1", "Alpha", "draft"),
        league("449.l.2", "Beta", "postdraft"),
        league("449.l.3", "Gamma", "predraft"),
    ]);
    assert!(stored.iter().all(|l| l.platform == "yahoo"));
    assert_eq!(stored[0].league_id, "449.l.1");
    assert_eq!(stored[0].status.as_deref(), Some("drafting"));
    assert_eq!(stored[1].status.as_deref(), Some("in_season"));
    assert_eq!(stored[2].status.as_deref(), Some("pre_draft"));
}

/// The failure this prevents: a Yahoo command returned `Err`, the string
/// became a toast, the toast was dismissed, and nothing in the log said the
/// command had been called at all.
///
/// `sandboxed` is what keeps this off a developer's real login Keychain: the
/// secrets go in a file in the scratch data directory, where there are none,
/// so the command fails before any network call is made.
#[tokio::test]
async fn a_yahoo_command_that_fails_leaves_an_error_line_naming_it() {
    let (mut state, dir) = AppState::scratch("yahoo-log");
    state.yahoo = Arc::new(YahooState::sandboxed(YahooHosts::default()));
    let capture = crate::applog::Capture::start();
    let out = crate::applog::logged!(
        "yahoo_leagues",
        String::new(),
        yahoo_leagues_inner(&state).await
    );
    assert!(
        out.unwrap_err().contains("Yahoo is not set up"),
        "the sentence the user sees is unchanged"
    );
    assert!(
        capture.saw("ERROR yahoo_leagues failed: Yahoo is not set up"),
        "{:?}",
        capture.lines()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_sign_in_state_cannot_be_guessed_from_the_clock_and_the_process_id() {
    // The failure this prevents: the state was the time in nanoseconds, a
    // counter and the pid, all of which a page open in the same browser can
    // estimate closely enough to post a code of its own to the loopback
    // listener. Sixteen random bytes cannot be.
    let first = nonce().expect("the OS random source is readable");
    let second = nonce().expect("random");
    assert_ne!(first, second);
    assert_eq!(first.len(), 32, "{first} is not sixteen bytes of hex");
    // It goes in a URL query, so it has to survive one unescaped.
    assert!(
        first.chars().all(|c| c.is_ascii_hexdigit()),
        "{first} is not URL-safe"
    );
    // Two states drawn a moment apart share no long prefix, which a
    // clock-derived one always did.
    let shared = first
        .chars()
        .zip(second.chars())
        .take_while(|(a, b)| a == b)
        .count();
    assert!(
        shared < 8,
        "{first} and {second} share {shared} leading characters"
    );
}

/// A file store that counts its writes. Nothing here goes near the login
/// Keychain: the file lives in a scratch directory of the test's own.
struct CountingStore {
    inner: crate::yahoo_secrets::FileStore,
    writes: std::sync::atomic::AtomicUsize,
}

impl crate::yahoo_secrets::SecretStore for CountingStore {
    fn read(&self, item: crate::yahoo_secrets::Item) -> Option<String> {
        self.inner.read(item)
    }
    fn write(&self, item: crate::yahoo_secrets::Item, value: &str) -> Result<(), String> {
        self.writes
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.write(item, value)
    }
    fn clear(&self, item: crate::yahoo_secrets::Item) -> Result<(), String> {
        self.inner.clear(item)
    }
}

#[tokio::test]
async fn an_unchanged_token_pair_is_not_written_back_on_every_tick() {
    // The failure this prevents: the draft poller persisted the pair after
    // every tick, and every persist was a `security` subprocess against the
    // login Keychain, three seconds apart, all evening, for a pair that
    // changes once an hour.
    let (state, dir) = AppState::scratch("yahoo-persist-unchanged");
    let yahoo = YahooState::sandboxed(YahooHosts::default());
    let store = Arc::new(CountingStore {
        inner: crate::yahoo_secrets::FileStore::in_dir(&dir),
        writes: std::sync::atomic::AtomicUsize::new(0),
    });
    let tokens = crate::yahoo_oauth::TokenSet {
        access_token: "access-1".into(),
        refresh_token: "refresh-1".into(),
        expires_at: u64::MAX,
    };
    crate::yahoo_secrets::save_tokens(store.as_ref(), &tokens).expect("the pair is stored");
    let client = crate::yahoo::YahooClient::with_hosts(
        crate::yahoo_oauth::YahooCredentials {
            client_id: "dj0yJmk9unit".into(),
            client_secret: "unit-secret".into(),
        },
        tokens,
        yahoo.hosts.clone(),
    );
    for _ in 0..3 {
        super::persist_tokens_into(store.clone(), &yahoo, &client).await;
    }
    assert_eq!(
        store.writes.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "only the write that stored the pair in the first place"
    );
    drop(state);
    let _ = std::fs::remove_dir_all(&dir);
}
