# Fuzz targets

Coverage-guided fuzz targets for the surfaces driven by data this app does not
control.

| Target | What it fuzzes | Why it matters |
|---|---|---|
| `sleeper_payloads` | Deserializing arbitrary bytes as `League`, `Draft`, `Pick`, `PlayerMeta`, `ProjectionRow` | The projections endpoint is undocumented; upstream payload drift is the realistic way draft night breaks |
| `board_assembly` | The whole ingestion pipeline — parse, score, rank, tier, replacement levels — with invariants (no duplicate players, finite points/VORP, 1-based ranks) | A malformed payload must degrade to warnings, never panic |
| `draft_math` | `slot_for_pick`, `picks_for_slot`, `survival_probability` | `overflow-checks` is on in release, so an underflow here is a live crash mid-draft |

## Status: builds, does not run on this machine

**The targets compile and are checked in, but the libFuzzer runtime does not
execute on macOS 27 / arm64 with the pinned toolchain.** The binaries start,
spin at 100% CPU, and never reach libFuzzer's driver or emit its startup
banner. Two constraints combine to cause this:

- `cargo-fuzz` is pinned to **0.12.0**, because current versions pull
  `cargo-platform 0.3.3`, which requires rustc 1.91. The newest toolchain
  installed here is nightly 1.90.
- The fuzz crate needs `default-features = false` on `draft-assistant`, because
  Tauri's `generate_handler!` expansion does not resolve under the fuzz
  workspace's build settings. That is why `lib.rs` gates the desktop shell
  behind the `desktop` feature.

To retry once the toolchain moves past 1.91:

```bash
rustup update nightly
cargo install cargo-fuzz --locked          # unpin from 0.12.0
cd src-tauri
cargo +nightly fuzz build
cargo +nightly fuzz run draft_math -- -max_total_time=60
```

Crashes land in `fuzz/artifacts/<target>/`; reproduce one with
`cargo +nightly fuzz run <target> fuzz/artifacts/<target>/<crash-file>`.

## What runs today instead

`src-tauri/tests/properties.rs` carries the same invariants as randomized
property tests via `proptest`, including a `parsing_robustness` module that
throws JSON-shaped and arbitrary-byte input at every Sleeper deserializer. That
runs on every `bun run test` and in CI, on stable, with no nightly requirement.

It is weaker than coverage-guided fuzzing — it does not evolve a corpus toward
new code paths — but it is not theoretical: the property suite is what found
`teams: 0` panicking `build_view` via `slot_for_pick` and the
`current_pick - 1` underflow.
