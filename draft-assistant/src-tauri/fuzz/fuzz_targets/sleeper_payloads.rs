#![no_main]
//! Deserializing an arbitrary byte string as each Sleeper type must either
//! succeed or return an error — never panic. Sleeper's projection endpoint is
//! undocumented and can change shape without notice.

use draft_assistant_lib::sleeper::{Draft, League, PickMeta, PlayerMeta, ProjectionRow};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let _ = serde_json::from_str::<League>(text);
    let _ = serde_json::from_str::<Draft>(text);
    let _ = serde_json::from_str::<ProjectionRow>(text);
    let _ = serde_json::from_str::<PlayerMeta>(text);
    let _ = serde_json::from_str::<PickMeta>(text);
    let _ = serde_json::from_str::<Vec<ProjectionRow>>(text);
});
