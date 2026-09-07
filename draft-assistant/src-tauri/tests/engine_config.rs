//! Engine persistence: config save/load with backup fallback, and manual-pick
//! storage with sanitized cache filenames. Everything here is disk-only.

use draft_assistant_lib::engine::{AppConfig, Engine, StoredLeague};
use draft_assistant_lib::picks::ManualPickStore;
use draft_assistant_lib::sleeper::Pick;
use std::path::PathBuf;

fn test_dir(label: &str) -> PathBuf {
    let unique = format!(
        "draft-assistant-engine-config-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    std::env::temp_dir().join(unique)
}

fn pick(pick_no: u32, player_id: &str) -> Pick {
    Pick {
        round: 1,
        pick_no,
        draft_slot: pick_no,
        player_id: player_id.into(),
        picked_by: None,
        metadata: None,
        is_keeper: None,
    }
}

// Note: no config in these tests ever carries `anthropic_api_key`, so the
// Keychain migration path (which shells out to /usr/bin/security) stays cold.
fn config_without_key() -> AppConfig {
    AppConfig {
        my_user_id: Some("user-1".into()),
        active_league_id: Some("league-1".into()),
        leagues: vec![StoredLeague {
            league_id: "league-1".into(),
            name: "My League".into(),
            season: "2025".into(),
            status: None,
            platform: "sleeper".into(),
        }],
        anthropic_api_key: None,
        chat_provider: Some("api".into()),
        chat_budget_usd: None,
        chat_spend_usd: Default::default(),
        device_name: None,
        companion_port: None,
        companion_enabled: false,
        log_level: None,
    }
}

#[test]
fn load_config_defaults_when_nothing_is_on_disk() {
    let dir = test_dir("empty");
    let engine = Engine::new(dir.clone());
    let config = engine.load_config();
    assert!(config.my_user_id.is_none());
    assert!(config.active_league_id.is_none());
    assert!(config.leagues.is_empty());
    assert!(config.chat_provider.is_none());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn config_round_trips_and_keeps_a_backup() {
    let dir = test_dir("roundtrip");
    let engine = Engine::new(dir.clone());

    engine.save_config(&config_without_key()).unwrap();
    let loaded = engine.load_config();
    assert_eq!(loaded.my_user_id.as_deref(), Some("user-1"));
    assert_eq!(loaded.active_league_id.as_deref(), Some("league-1"));
    assert_eq!(loaded.leagues.len(), 1);
    assert_eq!(loaded.leagues[0].name, "My League");
    assert_eq!(loaded.chat_provider.as_deref(), Some("api"));
    assert!(
        !dir.join("config.json.bak").exists(),
        "no backup until a second save replaces the first"
    );

    let mut second = config_without_key();
    second.my_user_id = Some("user-2".into());
    engine.save_config(&second).unwrap();
    assert!(dir.join("config.json.bak").exists());
    assert_eq!(engine.load_config().my_user_id.as_deref(), Some("user-2"));

    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn config_file_is_private_to_the_user() {
    use std::os::unix::fs::PermissionsExt;
    let dir = test_dir("perms");
    let engine = Engine::new(dir.clone());
    engine.save_config(&config_without_key()).unwrap();
    let mode = std::fs::metadata(dir.join("config.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn corrupt_config_falls_back_to_backup_then_default() {
    let dir = test_dir("corrupt");
    let engine = Engine::new(dir.clone());

    engine.save_config(&config_without_key()).unwrap();
    let mut updated = config_without_key();
    updated.my_user_id = Some("user-2".into());
    engine.save_config(&updated).unwrap(); // config.json = user-2, bak = user-1

    std::fs::write(dir.join("config.json"), "{ not json").unwrap();
    assert_eq!(
        engine.load_config().my_user_id.as_deref(),
        Some("user-1"),
        "unreadable live file falls back to the last good copy"
    );

    std::fs::write(dir.join("config.json.bak"), "also not json").unwrap();
    assert!(
        engine.load_config().my_user_id.is_none(),
        "both copies corrupt yields a default config, not a crash"
    );

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn manual_picks_round_trip_per_draft() {
    let dir = test_dir("manual");
    let engine = Engine::new(dir.clone());

    assert!(engine.load_manual_picks("draft-a").is_empty());
    engine
        .save_manual_picks("draft-a", &[pick(1, "p1"), pick(2, "p2")])
        .unwrap();
    engine
        .save_manual_picks("draft-b", &[pick(1, "px")])
        .unwrap();

    let a = engine.load_manual_picks("draft-a");
    assert_eq!(a.len(), 2);
    assert_eq!(a[1].player_id, "p2");
    assert_eq!(engine.load_manual_picks("draft-b").len(), 1);
    assert!(engine.load_manual_picks("draft-c").is_empty());

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn manual_pick_cache_filenames_are_sanitized_against_traversal() {
    let dir = test_dir("sanitize");
    let engine = Engine::new(dir.clone());

    let hostile_id = "../../evil id/☃!x";
    engine
        .save_manual_picks(hostile_id, &[pick(1, "p1")])
        .unwrap();

    // Loading back through the same hostile id works…
    assert_eq!(engine.load_manual_picks(hostile_id).len(), 1);
    // …because only [A-Za-z0-9_-] survive into the filename.
    assert!(dir.join("manual_picks_evilidx.json").is_file());
    // Nothing escaped the data dir.
    let entries: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(entries, ["manual_picks_evilidx.json"]);
    assert!(!dir.parent().unwrap().join("evil id").exists());

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn save_manual_picks_reports_disk_failures() {
    // Point the engine at a path that is a *file*, so the data dir cannot
    // exist and the atomic write must fail loudly instead of silently.
    let blocker = test_dir("blocked");
    std::fs::write(&blocker, "occupied").unwrap();
    let engine = Engine::new(blocker.clone());

    let err = engine
        .save_manual_picks("draft-a", &[pick(1, "p1")])
        .unwrap_err();
    assert!(err.contains("write"), "unexpected error: {err}");
    assert!(engine.load_manual_picks("draft-a").is_empty());

    std::fs::remove_file(blocker).unwrap();
}

#[test]
fn save_config_reports_disk_failures() {
    // Point the engine at a path that is a *file*, so its data dir cannot
    // exist and the save must say so instead of pretending it worked.
    let blocker = test_dir("config-blocked");
    std::fs::write(&blocker, "occupied").unwrap();
    let engine = Engine::new(blocker.clone());

    let err = engine.save_config(&config_without_key()).unwrap_err();
    assert!(err.contains("settings"), "unexpected error: {err}");
    assert!(engine.load_config().leagues.is_empty(), "nothing was saved");

    std::fs::remove_file(blocker).unwrap();
}

/// A config written before Yahoo existed has no `platform` on its leagues.
/// It has to keep loading, and every league in it is a Sleeper one, because
/// that is what it was.
#[test]
fn a_config_from_before_yahoo_loads_with_every_league_read_as_sleepers() {
    let dir = test_dir("platform-default");
    let engine = Engine::new(dir.clone());
    std::fs::write(
        dir.join("config.json"),
        r#"{"my_user_id": "user-1", "active_league_id": "league-1",
            "leagues": [{"league_id": "league-1", "name": "My League",
                         "season": "2025", "status": "in_season"}]}"#,
    )
    .expect("write a pre-Yahoo config");
    let config = engine.load_config();
    assert_eq!(config.leagues.len(), 1);
    assert_eq!(config.leagues[0].platform, "sleeper");
    std::fs::remove_dir_all(&dir).ok();
}

/// And one written since round-trips whichever platform it names.
#[test]
fn a_platform_survives_a_save_and_a_load() {
    let dir = test_dir("platform-round-trip");
    let engine = Engine::new(dir.clone());
    let mut config = config_without_key();
    config.leagues.push(StoredLeague {
        league_id: "449.l.12345".into(),
        name: "Wire Wednesday".into(),
        season: "2026".into(),
        status: Some("drafting".into()),
        platform: "yahoo".into(),
    });
    engine.save_config(&config).expect("save the config");

    let back = Engine::new(dir.clone()).load_config();
    let platforms: Vec<&str> = back
        .leagues
        .iter()
        .map(|league| league.platform.as_str())
        .collect();
    assert_eq!(platforms, ["sleeper", "yahoo"]);
    assert_eq!(back.leagues[1].league_id, "449.l.12345");
    std::fs::remove_dir_all(&dir).ok();
}

/// A key left in the config file from before Keychain storage is moved out
/// on load. Into *this engine's* store: an engine over a scratch directory
/// has a file store in that directory, and the migration must land there
/// rather than in the login Keychain of whoever runs the tests. Before the
/// store was fixed at construction, exactly this test would have overwritten
/// the developer's real key.
#[test]
fn a_key_in_a_test_engines_config_migrates_into_its_file_store_not_the_keychain() {
    use draft_assistant_lib::secrets::load_from;
    use draft_assistant_lib::yahoo_secrets::FileStore;

    let dir = test_dir("key-migration");
    std::fs::create_dir_all(&dir).unwrap();
    const KEY: &str = "sk-ant-api03-test-key-that-must-stay-in-the-scratch-dir";
    std::fs::write(
        dir.join("config.json"),
        format!(r#"{{"my_user_id": "user-1", "anthropic_api_key": "{KEY}"}}"#),
    )
    .unwrap();

    let engine = Engine::new(dir.clone());
    let config = engine.load_config();
    assert!(
        config.anthropic_api_key.is_none(),
        "the key should have left the config"
    );
    assert_eq!(config.my_user_id.as_deref(), Some("user-1"));

    // It went into the file store in the scratch directory...
    let store = FileStore::in_dir(&dir);
    assert_eq!(load_from(&store).as_deref(), Some(KEY));
    assert_eq!(
        engine.secret_store().and_then(load_from).as_deref(),
        Some(KEY),
        "the engine's own store is that file"
    );
    // ...and the rewritten config file no longer carries it.
    let on_disk = std::fs::read_to_string(dir.join("config.json")).unwrap();
    assert!(!on_disk.contains(KEY), "the key is still in config.json");
    assert!(
        std::fs::read_to_string(dir.join("yahoo-secrets.json"))
            .unwrap()
            .contains(KEY),
        "the key should be in the scratch directory's secrets file"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A machine with no Keychain has no store but the config file, and the
/// key stays there rather than vanishing.
#[test]
fn with_no_secret_store_the_key_stays_in_the_config_file() {
    use draft_assistant_lib::sleeper::SleeperClient;

    let dir = test_dir("key-no-store");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.json"),
        r#"{"anthropic_api_key": "sk-ant-api03-stays-put"}"#,
    )
    .unwrap();
    let engine = Engine::with_secrets(dir.clone(), SleeperClient::new(), None);
    assert!(engine.secret_store().is_none());
    let config = engine.load_config();
    assert_eq!(
        config.anthropic_api_key.as_deref(),
        Some("sk-ant-api03-stays-put")
    );
    assert!(!dir.join("yahoo-secrets.json").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `Engine::for_app` and the default `YahooState` are the two constructors
/// that can reach the machine's Keychain. Neither belongs in a test: this
/// reads every test file in the crate so one cannot quietly come back. The
/// needles are assembled at runtime so this test's own text does not trip it.
#[test]
fn no_test_builds_an_engine_or_yahoo_state_over_the_real_keychain() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    collect_rust_files(&root.join("tests"), &mut files);
    collect_rust_files(&root.join("src"), &mut files);
    let needles = [
        format!("Engine::{}(", "for_app"),
        format!("YahooState::{}()", "default"),
        format!("YahooState::{}(", "new"),
    ];
    let mut offenders = Vec::new();
    for file in files {
        let under_tests = file.starts_with(root.join("tests"));
        let is_test_module = file
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with("_tests.rs"));
        if !under_tests && !is_test_module {
            continue;
        }
        let text = std::fs::read_to_string(&file).expect("read a test source file");
        for needle in &needles {
            if text.contains(needle.as_str()) {
                offenders.push(format!("{}: {needle}", file.display()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "these tests can reach the real Keychain; use Engine::new / YahooState::sandboxed:\n{}",
        offenders.join("\n")
    );
}

fn collect_rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}
