# Top 15 fixes — progress

From the 2026-08-30 audit (`.claude/grade-report.md`). Updated as each lands.

| # | Item | What it is | Status |
|---|------|-----------|--------|
| 1 | B1 | App can crash mid-draft (divide by zero on Sleeper data) | **done** — `assemble()` refuses a draft reporting 0 teams/rounds; `slot_for_pick` returns `Option` instead of dividing by zero |
| 2 | B2 | Lies about live scores when the network dies | **done** — `refresh_live` returns `Err` and leaves the staleness clock alone when all three endpoints fail; partial success still counts as fresh |
| 3 | B3 | Freezes on load (15 MB parse on the main thread) | **done** — `*_off_thread` cache helpers run reads/writes on `spawn_blocking`; `projections.rs` rewired |
| 4 | B4 | One hiccup blanks a week; 33 requests run one-at-a-time | **done** — `get_json` retries 3× with backoff on transport errors and 5xx (never 404); weekly projections and the matchup sweep now run 6-at-a-time |
| 5 | D1 | Nothing tests how Sleeper data is read | **done** — `tests/wire_parsing*.rs`; Sleeper parsing now covered |
| 6 | D2 | Nothing tests the season screen math | **done** — `tests/season_view*.rs`, `engine_*.rs`; Rust coverage 57.3% → 70.9%, 106 → 173 tests |
| 7 | B5 | Manual picks accept players that don't exist | **done** — `record_manual_pick` rejects any id not on the league board |
| 8 | G5 | Recalculates everything every 30 seconds | **done** — `SeasonAnalysis` (standings/odds, waivers, trades) is computed once and reused by the poller via `build_season_view_cached`, rebuilt every ~10 min so league moves still land |
| 9 | G6 | All 200 board rows redraw every 3 seconds | **done** — `BoardRow` is memoised and the draft handler is a stable `useCallback`, so unchanged rows skip the poll re-render |
| 10 | G1+G2 | Waiver math repeats itself ~780× per refresh | **done** — rival rosters and their baselines are built once outside the free-agent loop; `marginal_gain` takes the baseline instead of recomputing it |
| 11 | C3 | Zoomed pictures trap keyboard users | **done** — focus moves to Close on open, Tab is trapped inside the dialog, and focus returns to the thumbnail on close |
| 12 | C2 | Season tabs ignore arrow keys | **done** — arrow/Home/End navigation, roving tabindex, and real `tabpanel`s wired to their tabs with `aria-controls`/`aria-labelledby` |
| 13 | C1 | Sort arrows invisible to screen readers | **done** — `aria-sort` was inert on a button, so the state moved into the accessible name ("Proj, sorted descending") |
| 14 | I1 | CI wastes 10–20 min per push at 10× cost | **done** — `Swatinem/rust-cache@v2` added to `verify.yml` |
| 15 | H1+H2 | README describes an app that no longer exists | **done** — season screen and Ask Claude documented, full 40-file layout, both fixtures' regeneration commands, plus a new root README |

## Notes
- The eight Rust test files from the killed agent were salvaged: a raw-string delimiter bug in `wire_parsing.rs`, `unwrap_err()` on non-`Debug` types in `engine_offline.rs`, and one wrong expectation (`FLEX` makes TE draftable, so the app was right and the test was wrong).

## Result

All 15 done. `npm run verify` exits 0.

| | before | after |
|---|---|---|
| Rust tests | 106 | 182 |
| Rust line coverage | 57.3% | 71.5% |
| Frontend tests | 81 | 117 |
| Frontend line coverage | 71.8% | 88.6% |

Verified in the running app: both screens, all six season tabs, arrow-key tab
navigation, and the zoom overlay.

### Picked up along the way
- `coverage/` was being walked by both the LOC checker and ESLint; excluded from each.
- The cache envelope, its parse, and the atomic write moved out of `engine.rs`
  into `cache.rs` — three copies of serialize-tmp-rename collapsed into one, and
  it kept `engine.rs` under the 500-line cap.
- `SeasonAnalysis` sits in `season_view_parts.rs` (re-exported from `season.rs`)
  for the same reason.
