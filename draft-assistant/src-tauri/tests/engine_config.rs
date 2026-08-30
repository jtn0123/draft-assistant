//! Engine persistence: config save/load with backup fallback, and manual-pick
//! storage with sanitized cache filenames. Everything here is disk-only.

use draft_assistant_lib::engine::{AppConfig, Engine, StoredLeague};
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
        }],
        anthropic_api_key: None,
        chat_provider: Some("api".into()),
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

    engine.save_config(&config_without_key());
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
    engine.save_config(&second);
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
    engine.save_config(&config_without_key());
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

    engine.save_config(&config_without_key());
    let mut updated = config_without_key();
    updated.my_user_id = Some("user-2".into());
    engine.save_config(&updated); // config.json = user-2, bak = user-1

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
