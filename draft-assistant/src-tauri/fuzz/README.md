# Fuzz targets

Coverage-guided fuzz targets for the surfaces driven by data this app does not
control.

| Target | What it fuzzes | Why it matters |
|---|---|---|
| `sleeper_payloads` | Deserializing arbitrary bytes as `League`, `Draft`, `PlayerMeta`, `PickMeta`, `ProjectionRow` | The projections endpoint is undocumented; upstream payload drift is the realistic way draft night breaks |
| `board_assembly` | The whole ingestion pipeline -- parse, score, rank, tier, replacement levels -- with invariants (no duplicate players, finite points and VORP, 1-based ranks) | A malformed payload must degrade to warnings, never panic |
| `draft_math` | `slot_for_pick`, `market_pick`, `survival_probability_in` | `overflow-checks` is on in release, so an underflow here is a live crash mid-draft |

## Running them

Needs nightly and `cargo-fuzz`:

```bash
rustup update nightly
cargo install cargo-fuzz --locked
cd src-tauri/fuzz
cargo +nightly fuzz run -s none draft_math -- -max_total_time=60
```

`-s none` turns the sanitizer off, and it is not optional here: with
AddressSanitizer on, linking fails in `tauri-utils` with `ld: initializer
pointer has no target`. The library still links `tauri` for the types its
command modules use, even with the app shell compiled out, so the only way to
keep ASan would be to make `tauri` itself optional across every
`#[tauri::command]` module. That is a large change to the shipped app for a
tool that already finds what it is pointed at: without ASan libFuzzer still
catches every panic, assertion failure, overflow and out-of-bounds index,
which is what all three targets assert on. What it loses is heap corruption
in unsafe code, and this crate has none.

Crashes land in `fuzz/artifacts/<target>/`. Reproduce one with:

```bash
cargo +nightly fuzz run -s none <target> fuzz/artifacts/<target>/<crash-file>
```

Last run: about 200k executions per target, no crashes.

## How this builds

The targets take the library with `default-features = false`, which compiles
out the `desktop` feature -- the Tauri application shell at the bottom of
`src/lib.rs`. `generate_context!` does not expand under cargo-fuzz's build
settings, and nothing fuzzed here needs the app. Every ordinary build, test
and clippy run has the feature on, so the shell is still covered.

The fuzz crate is not part of the main package, so `cargo clippy
--all-targets` from `src-tauri` does not reach it. Lint it directly:

```bash
cd src-tauri/fuzz && cargo +nightly clippy -- -D warnings
```

## What runs on every commit instead

`src-tauri/tests/properties.rs` carries the same invariants as randomized
property tests via `proptest`, including a `parsing_robustness` module that
throws JSON-shaped and arbitrary-byte input at every Sleeper deserializer.
That runs on stable, in the ordinary test suite, with no nightly requirement.

It is weaker than coverage-guided fuzzing -- it does not evolve a corpus
toward new code paths -- but it is not theoretical: the property suite is what
found `teams: 0` panicking `build_view` through `slot_for_pick`, and the
`current_pick - 1` underflow.
