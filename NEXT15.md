# Next 15 fixes — progress

Follow-on from `TOP15.md`, drawn from `.claude/grade-report.md`.

| # | Item | What it is | Status |
|---|------|-----------|--------|
| 1 | E2 | Pasted junk becomes a Sleeper URL | **done** — `extract_id` returns `Result` and rejects anything that is not a 15–25 digit run |
| 2 | E3 | One cache filename skips the safety filter | **done** — one shared `cache::safe_key` now used by all three cache filenames |
| 3 | E4 | Username unescaped in a URL, on an unpooled request | **done** — `SleeperClient::user` validates the username and goes through the pooled client |
| 4 | B6 | API key visible in the process list | **done** — key passed on stdin, not argv; op is an enum (no more `panic!`). Round-tripped against the real Keychain |
| 5 | E5 | Chat runs the first `claude` on PATH | **done** — known install paths first; the PATH fallback skips world-writable directories |
| 6 | G3 | Trade search allocates ~1M strings per refresh | **done** — sound upper bound prunes pairs before any lineup solve; a randomized test proves it matches brute force |
| 7 | G4 | Playoff odds run through hash lookups in the hot loop | **done** — slot-indexed vectors instead of HashMaps. Measured 39ms → 14ms, identical odds |
| 8 | G7 | Board re-filters and re-sorts every 3s for nothing | **partly** — comparator is now O(1) via decorate-sort-undecorate. Did **not** skip the recompute: a rebuilt board can carry the same players with new numbers, and caching there would render stale points |
| 9 | G8 | Photos cross to the UI as base64; no build target set | **partly** — `build.target`, sourcemaps and `npm run analyze` added. Base64→asset-protocol deferred: see note |
| 10 | A1 | `Engine` is assembled from five files | **done** — `ImageCache`, `SeasonLoader`, `HistoryStore` traits declare each seam; `Engine`'s doc comment lists them all |
| 11 | A6+C5 | `App.tsx` is the app's brain | **done** — `useSeasonSession` hook + `prefs.ts` store; App.tsx 499→466 lines, 19→16 useState, and the season lifecycle now has 6 tests of its own |
| 12 | A3+D6 | Poll loops buried in command handlers, untested | **done** — `poll.rs` holds the tick decisions (`DraftPollMemory`, `LiveEmitGate`, `AnalysisCache`, `record_poll_outcome`) with 9 tests; both loops call into it |
| 13 | C7 | Ten unscoped stylesheets can silently collide | **done** — found exactly **one** real cross-file collision (`.board-row`), moved it home, and added `scripts/check-css.mjs` to the gate so another cannot appear |
| 14 | I2+I3+I6 | Slow checks first, no TS formatter, no pre-commit hook | **done** — `verify:fast` runs the cheap checks first; prettier added for TS/CSS; `.githooks/pre-commit` enabled |
| 15 | I4+I5+F1+F2+F3+D5 | No pinning, no audit job, duplicate HTTP stack, no Rust coverage gate | **done** — `rust-toolchain.toml` + `.nvmrc` + `engines`; CI reads both; `cargo audit` + `npm audit` job; Rust coverage floor at 68% |

## Result

13 of 15 fully done, 2 partly — with reasons.

| | before | after |
|---|---|---|
| Rust tests | 182 | 197 |
| Rust line coverage | 71.5% | 72.3% |
| Frontend tests | 117 | 123 |
| Frontend line coverage | 88.6% | 88.6% |

`npm run verify` exits 0. Verified in the **real desktop app**, not just the
browser preview: manager avatars and team logos render under the new CSP, live
sync reports green, and the season loads through the new retry and parallel
sweep.

### Measured
- Playoff-odds simulation: **39ms → 14ms**, identical odds.
- Trade search: most candidate pairs now die on a bounds check before any
  lineup solve. A randomized test over 40 rosters proves the pruned search
  finds exactly what brute force finds.
- Keychain: store/load/clear round-tripped against the real macOS Keychain
  with the key on stdin instead of argv.

### The two partials, and why
- **#8 (board re-sorts every 3s).** The comparator was the real cost and is
  fixed. Skipping the recompute entirely would have been wrong: "Refresh data"
  can rebuild the board with the same players and new projections, and a cache
  keyed on the player list would then render stale points.
- **#9 (photos as base64).** Build target, sourcemaps and `npm run analyze`
  landed. Moving images to Tauri's `asset:` protocol did not — it changes how
  every picture in the app is delivered, and the benefit (~33% less data on a
  local bridge, for images already fetched once per session) does not justify
  that risk right now.

### Two audit findings that turned out to be overstated
- **Duplicate HTTP stack.** `reqwest` 0.13 only enters on targets this app does
  not ship to; on macOS `cargo tree` shows exactly one. Bumping to match pulls
  in a different rustls/aws-lc-rs chain for no benefit, so 0.12.28 is now
  pinned with that reasoning recorded in `Cargo.toml`.
- **"Ten unscoped stylesheets can silently collide."** There was exactly **one**
  real cross-file collision (`.board-row`). It is fixed, and
  `scripts/check-css.mjs` now fails the build if another appears — much better
  value than a CSS-Modules rewrite that would have broken the deliberate
  cross-file selectors this codebase uses.
