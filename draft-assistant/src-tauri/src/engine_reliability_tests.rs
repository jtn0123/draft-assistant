//! The engine's cache fallback: an expired cache file is refused by the
//! freshness read but still there for the outage path.

use super::*;
use crate::cache::Cached;

fn test_dir(label: &str) -> PathBuf {
    let unique = format!(
        "draft-assistant-{label}-{}-{}",
        std::process::id(),
        now_secs()
    );
    std::env::temp_dir().join(unique)
}

#[test]
fn expired_cache_is_still_available_for_outage_fallback() {
    let dir = test_dir("stale-cache");
    let engine = Engine::new(dir.clone());
    let cached = Cached {
        fetched_at: 1,
        data: vec![10_u32, 20_u32],
    };
    std::fs::write(
        engine.cache_path("test.json"),
        serde_json::to_string(&cached).unwrap(),
    )
    .unwrap();

    assert!(engine.read_cache::<Vec<u32>>("test.json", 1).is_none());
    assert_eq!(
        engine.read_cache_any::<Vec<u32>>("test.json"),
        Some((1, vec![10, 20]))
    );

    std::fs::remove_dir_all(dir).unwrap();
}

/// A cache write that fails used to be discarded entirely: every fetch
/// worked, nothing was ever written, and the only symptom of a full disk or
/// a read-only data directory was a slow cold start, every launch, forever.
#[cfg(unix)]
#[test]
fn a_cache_write_that_fails_is_remembered_as_a_warning() {
    use std::os::unix::fs::PermissionsExt;
    let dir = test_dir("readonly-cache");
    let engine = Engine::new(dir.clone());
    assert!(engine.take_cache_warnings().is_empty());

    // A data directory nobody may write to, which is what a full disk or a
    // botched permission repair looks like from in here.
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    engine.write_cache("players.json", &vec![1u32, 2]);
    engine.write_cache("players.json", &vec![1u32, 2]);

    let warnings = engine.take_cache_warnings();
    assert_eq!(
        warnings.len(),
        1,
        "one message per failure, not per attempt"
    );
    assert!(
        warnings[0].contains("players.json"),
        "the warning should name the file: {}",
        warnings[0]
    );
    // Drained, so the next load does not repeat the last one's warnings.
    assert!(engine.take_cache_warnings().is_empty());

    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

/// Every cache write goes through a temp file of its own now; none of them
/// may be left behind, and a stale one from an earlier run is swept when the
/// engine opens the directory.
#[test]
fn an_engine_sweeps_temp_files_an_earlier_run_left_behind() {
    let dir = test_dir("sweep-on-start");
    std::fs::create_dir_all(&dir).unwrap();
    let stale = dir.join("players.json.999.0.tmp");
    std::fs::write(&stale, "half a file").unwrap();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(48 * 3600);
    std::fs::File::options()
        .write(true)
        .open(&stale)
        .unwrap()
        .set_modified(old)
        .unwrap();

    let engine = Engine::new(dir.clone());
    assert!(!stale.exists(), "the leftover temp file should be swept");

    engine.write_cache("players.json", &vec![7u32]);
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "tmp"))
        .collect();
    assert!(leftovers.is_empty(), "left behind {leftovers:?}");
    assert_eq!(
        engine.read_cache_any::<Vec<u32>>("players.json").unwrap().1,
        vec![7u32]
    );
    std::fs::remove_dir_all(dir).unwrap();
}
