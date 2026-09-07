#![no_main]
//! The clock math runs on every poll with values taken from the payload.
//! `overflow-checks` is on in release, so an underflow here is a live crash.

use draft_assistant_lib::draft::{self, slot_for_pick, survival_probability_in, DraftOrder};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 16 {
        return;
    }
    let u32_at = |i: usize| u32::from_le_bytes(data[i..i + 4].try_into().unwrap());
    let pick = u32_at(0);
    // Bound the search space: the ownership walk covers teams*rounds, so
    // unbounded values fuzz the allocator rather than the logic.
    let teams = u32_at(4) % 64;
    let rounds = u32_at(12) % 64;
    let order = DraftOrder {
        linear: data[0] & 1 == 1,
        reversal_round: u32::from(data[1] % 8),
    };

    match slot_for_pick(pick, teams, order) {
        Some(slot) => {
            assert!(teams > 0 && pick > 0, "a pick nobody makes named slot {slot}");
            assert!((1..=teams).contains(&slot), "slot {slot} outside 1..={teams}");
        }
        None => assert!(teams == 0 || pick == 0, "pick {pick} of {teams} teams has no owner"),
    }

    // Every pick on the board maps back to the slot that owns it.
    for p in 1..=teams.saturating_mul(rounds).min(4096) {
        let slot = slot_for_pick(p, teams, order).expect("a pick on the board has an owner");
        assert!((1..=teams).contains(&slot), "pick {p} maps to slot {slot}");
    }

    // A keeper never moves a pick later in the market it is measured against.
    let keepers: std::collections::HashSet<u32> =
        (0..data[2] % 32).map(|i| u32_at(8) % 200 + u32::from(i)).collect();
    let at_pick = u32_at(8) % 1000;
    let market = draft::market_pick(at_pick, &keepers);
    assert!(market <= at_pick.max(1), "market pick {market} is past overall {at_pick}");

    // ADP arrives as a JSON number, so it is always finite: scale into the
    // range Sleeper actually reports rather than fuzzing NaN bit patterns no
    // payload can express.
    let adp = f64::from(u32_at(0) % 100_000) / 100.0;
    let p = survival_probability_in(adp, u32_at(4) % 1000, teams);
    assert!(p.is_finite(), "survival({adp}) was {p}");
    assert!((0.0..=1.0).contains(&p), "survival({adp}) = {p} outside [0,1]");
});
