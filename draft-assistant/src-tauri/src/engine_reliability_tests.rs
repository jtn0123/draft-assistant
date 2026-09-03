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
