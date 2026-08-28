#![no_main]
//! The clock math runs on every poll with values taken from the payload.
//! `overflow-checks` is on in release, so an underflow here is a live crash.

use draft_assistant_lib::draft::{picks_for_slot, slot_for_pick, survival_probability};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 16 {
        return;
    }
    let u32_at = |i: usize| u32::from_le_bytes(data[i..i + 4].try_into().unwrap());
    let pick = u32_at(0);
    let teams = u32_at(4) % 64;
    let slot = u32_at(8) % 64;
    // Bound the search space: picks_for_slot walks teams*rounds, so unbounded
    // values fuzz the allocator rather than the logic.
    let rounds = u32_at(12) % 64;

    let s = slot_for_pick(pick, teams);
    assert!(s >= 1, "slot_for_pick({pick}, {teams}) returned {s}");
    if teams > 0 {
        assert!(s <= teams, "slot {s} exceeds {teams} teams");
    }

    for p in picks_for_slot(slot, teams, rounds) {
        assert!(p >= 1 && p <= teams.saturating_mul(rounds));
        assert_eq!(slot_for_pick(p, teams), slot, "pick {p} maps to the wrong slot");
    }

    // ADP arrives as a JSON number, so it is always finite — scale into the
    // range Sleeper actually reports rather than fuzzing NaN bit patterns that
    // no payload can express.
    let adp = f64::from(u32_at(0) % 100_000) / 100.0;
    let p = survival_probability(adp, pick % 1000);
    assert!(p.is_finite(), "survival({adp}) was {p}");
    assert!((0.0..=1.0).contains(&p), "survival({adp}) = {p} outside [0,1]");
});
