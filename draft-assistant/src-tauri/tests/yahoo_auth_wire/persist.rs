use super::*;

/// A file store that counts its writes, in a scratch directory of the test's
/// own. Nothing here goes near the login Keychain.
struct CountingStore {
    inner: draft_assistant_lib::yahoo_secrets::FileStore,
    writes: std::sync::atomic::AtomicUsize,
}

impl draft_assistant_lib::yahoo_secrets::SecretStore for CountingStore {
    fn read(&self, item: draft_assistant_lib::yahoo_secrets::Item) -> Option<String> {
        self.inner.read(item)
    }
    fn write(
        &self,
        item: draft_assistant_lib::yahoo_secrets::Item,
        value: &str,
    ) -> Result<(), String> {
        self.writes
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.write(item, value)
    }
    fn clear(&self, item: draft_assistant_lib::yahoo_secrets::Item) -> Result<(), String> {
        self.inner.clear(item)
    }
}

fn scratch_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "draft-assistant-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

#[tokio::test]
async fn the_pair_is_written_back_once_after_a_refresh_and_not_on_the_ticks_between() {
    // The failure this prevents: the draft poller persisted the pair after
    // every tick, and each persist was a `security` subprocess against the
    // login Keychain, three seconds apart, all evening. A refresh changes
    // the pair once an hour; that is the one write worth making.
    let stub = serve(move |request: &Request| {
        if request.path() == "/oauth2/get_token" {
            Reply::ok(FRESH_TOKEN)
        } else {
            Reply::ok(TEAMS)
        }
    });
    let dir = scratch_dir("yahoo-persist-refresh");
    let store = std::sync::Arc::new(CountingStore {
        inner: draft_assistant_lib::yahoo_secrets::FileStore::in_dir(&dir),
        writes: std::sync::atomic::AtomicUsize::new(0),
    });
    draft_assistant_lib::yahoo_secrets::save_tokens(store.as_ref(), &stale_tokens())
        .expect("the stale pair is stored");
    let yahoo = draft_assistant_lib::state::YahooState::sandboxed(hosts(&stub));
    let client = client_for(&stub, stale_tokens());
    let writes = || store.writes.load(std::sync::atomic::Ordering::SeqCst);

    // Nothing has changed yet: a tick that made no call writes nothing.
    draft_assistant_lib::commands_yahoo::persist_tokens_into(store.clone(), &yahoo, &client).await;
    assert_eq!(writes(), 1, "only the write that stored the pair");

    client.league_teams(LEAGUE_KEY).await.expect("teams load");
    draft_assistant_lib::commands_yahoo::persist_tokens_into(store.clone(), &yahoo, &client).await;
    assert_eq!(writes(), 2, "the refreshed pair is written once");
    let stored =
        draft_assistant_lib::yahoo_secrets::load_tokens(store.as_ref()).expect("a pair is stored");
    assert_eq!(stored.access_token, "access-2");
    assert_eq!(stored.refresh_token, "refresh-2");

    for _ in 0..5 {
        client.league_teams(LEAGUE_KEY).await.expect("teams load");
        draft_assistant_lib::commands_yahoo::persist_tokens_into(store.clone(), &yahoo, &client)
            .await;
    }
    assert_eq!(
        writes(),
        2,
        "the ticks between refreshes wrote the pair again"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_pair_with_no_refresh_token_is_cleared_rather_than_kept_as_connected() {
    // The stored half of the empty-refresh-token case: the client says the
    // grant is gone, and whoever persists the pair clears it, so the next
    // status says "connect again" rather than "Connected" over a pair that
    // can never work.
    let stub = serve(move |_: &Request| Reply::ok(TEAMS));
    let dir = scratch_dir("yahoo-persist-empty-refresh");
    let store = std::sync::Arc::new(CountingStore {
        inner: draft_assistant_lib::yahoo_secrets::FileStore::in_dir(&dir),
        writes: std::sync::atomic::AtomicUsize::new(0),
    });
    let broken = TokenSet {
        refresh_token: String::new(),
        ..stale_tokens()
    };
    draft_assistant_lib::yahoo_secrets::save_tokens(store.as_ref(), &broken).expect("stored");
    let yahoo = draft_assistant_lib::state::YahooState::sandboxed(hosts(&stub));
    let client = client_for(&stub, broken);
    let error = client
        .league_teams(LEAGUE_KEY)
        .await
        .expect_err("nothing to renew with");
    assert_eq!(error, YahooError::SignedOut, "{error:?}");
    assert!(stub.requests().is_empty(), "a call went out with no token");
    draft_assistant_lib::commands_yahoo::persist_tokens_into(store.clone(), &yahoo, &client).await;
    assert!(
        draft_assistant_lib::yahoo_secrets::load_tokens(store.as_ref()).is_none(),
        "the dead pair is still stored"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
