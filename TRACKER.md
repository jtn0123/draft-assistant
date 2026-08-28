# Work tracker — 2026-08-28 (draft day, 17:00 PDT)

Status legend: `todo` · `doing` · `done <commit>` · `deferred (why)`

Updated as work lands. Items come from the grade report (`.claude/grade-report.md`),
the dogfood report (`dogfood-output/report.md`), and the in-app Claude's own list of
what it could not see.

## Requested this session

| # | Item | Status | Notes |
|---|------|--------|-------|
| 1 | B6 — chat memory + configurable model | done (uncommitted) | `chat/prompt.rs` sends the last 6 exchanges + any summary; `DRAFT_ASSISTANT_CLAUDE_MODEL` default; stub-CLI tests assert history and flags |
| 2 | D2 — unit-test command-layer pure logic | done (uncommitted) | `manual.rs` (`apply_manual_pick` / `undo_manual_pick`, 5 tests, plus a new not-on-the-board guard); `sleeper::extract_id` (5 tests) |
| 3 | Chat: model selector (Opus / Sonnet / Fable) | done (uncommitted) | Settings fold in the panel; remembered in localStorage |
| 4 | Chat: speed selector (fast mode) | done (uncommitted) | `--settings '{"fastMode":true}'`; the account reports `extra_usage_disabled`, so the panel notes once "answered at standard speed" — enable extra usage to get it |
| 5 | Chat: thinking/effort selector | done (uncommitted) | `--effort low/medium/high/xhigh/max`, Default = CLI default |
| 6 | Chat: full app context (whole board, not top 40) | done (uncommitted) | all 419 players as a pipe table (~10k tokens vs 34k for JSON); state JSON keeps rosters, recent picks, baselines |
| 7 | Chat: web-search toggle, default off | done (uncommitted) | `--tools WebSearch` when on, `--tools ""` when off; system prompt tells the model when it may search |
| 8 | Chat: new-session button | done (uncommitted) | **New chat** in the panel header |
| 9 | Chat: token usage display | done (uncommitted) | usage line: context tokens, seconds, model, question count, session cost, web searches |
| 10 | Chat: compaction option with "takes a minute or two" warning | done (uncommitted) | **Compact** (enabled after 2 questions); warning in the tooltip and the pending line; summary replaces the thread as history |
| 11 | Board: click column headers to sort (Bye, Pos, Pts, VORP, Tier, ADP, Surv) | done (uncommitted) | every header sorts; second click flips; `#` restores value order; blanks last; `aria-sort` set; unit + E2E tests |
| 12 | Why `weekly_2026.json could not be cached (replace …: No such file)` | done (uncommitted) | Two loads overlapped (React dev StrictMode double-mount at launch) and shared one `weekly_2026.json.tmp`; the loser's rename found nothing. Fix: per-write tmp names (`store.rs`, 8-thread race test) + launch effect guarded to run once (`App.tsx`). Harmless in effect: the data was in memory and used. |
| 13 | Claude's wish list: full board / weekly data / live picks | done / deferred | full board: done (item 6); weekly cache: done (item 12); schedule/SoS/playoff weeks: deferred (schema change); live picks: already flow in |
| 14 | Dogfood ISSUE-001 — confirm modal ignores Escape, no focus | done (uncommitted) | native `<dialog>` (`ConfirmDialog.tsx`): focus lands on Confirm, Escape/backdrop cancel, focus returns to the row; E2E test |
| 15 | Dogfood ISSUE-002 — chat Escape lost after focus leaves | done 295d2c9 | |
| 16 | Dogfood ISSUE-003 — chat drawer covers header + decision columns | done (uncommitted) | panel is a sticky flex column beside the page; board scrolls horizontally if needed; E2E asserts nothing actionable sits under it |
| 17 | Dogfood ISSUE-004 — sticky alert covers a board row | done (uncommitted) | toast anchored top-right under the header |
| 18 | Dogfood ISSUE-005 — header wraps at 1000 px | done (uncommitted) | league name ellipsises, buttons wrap as whole units, pill never breaks mid-word |
| 19 | Dogfood ISSUE-006 — preseason QUESTIONABLE penalised in safe mode | done (uncommitted) | `recommend::serious_injury` (out/ir/pup/sus/doubtful/na/cov) gates the −15; badge muted for Questionable with a tooltip; tests both sides |
| 20 | Dogfood ISSUE-007 — preview greets with red alert | done (uncommitted) | `api.preview` skips live sync and shows a neutral banner; E2E asserts no alert on load |
| 21 | Write-up: crib sheet vs app, what AI adds, app vs Sleeper web | done | see the section at the bottom |
| 22 | Demo / replay mode (asked 05:25) | done (uncommitted) | `scripts/replay-sleeper.mjs` replays any completed draft as live; `DRAFT_ASSISTANT_SLEEPER_BASE` override in `sleeper.rs` (D1's seam); browser preview `?replay=<url>` polls dumps; README "Demo / replay mode" |
| 23 | Live test of the app via Chrome + mock draft | done differently | your logged-in Chrome is unreachable from this harness (no debug port; Chrome profile folder blocked by macOS). Tested instead against a live replay of the league's 2025 draft with you at slot 2 — 13 scenarios, all pass: `dogfood-output/replay/report.md` |
| 24 | R-1 sticky alert covered wrapped header buttons (regression from item 17) | done (uncommitted) | found by the replay test; failures are now an in-flow bar under the header, toasts are click-through |
| 25 | R-2 recommendation card shown after draft complete | done (uncommitted) | `build_view` returns no recommendations once `draft_over` |
| 26 | Bug hunt (asked 06:00) — 12 validated items H1–H12 | list delivered, awaiting go | `dogfood-output/bug-hunt-2026-08-28.md`; next: failing test → fix → passing → commit, per item |

## Data the chat could not see (from the in-app Claude)

- **Full board** — was capped at 40 of 419 (`chat.rs AVAILABLE_LIMIT`). → item 6.
- **Weekly projections uncached** — the cache write raced, not the fetch; the data
  was in memory and used for the board. → item 12. Schedule/SoS and playoff-week
  (15–17) points are not in `DraftView` at all; adding them is a schema change
  (`1.2` → `1.3`) — deferred until after the draft.
- **Live pick context** — `recent_picks` and `position_run` fill in as picks land;
  nothing to change, the chat already receives them on every question.

## Deferred (post-draft)

- Playoff-week / strength-of-schedule fields on `BoardPlayer` (schema bump).
- Remaining grade-report items: D1, B1, B5, G1, C1 (covered by item 14), I2 CI run.

## Verification (2026-08-28 ~04:10)

`bun run verify`: LOC cap ok · cargo fmt ok · tsc ok · vite build ok · Vitest 31/31 ·
Rust 54 lib + integration suites ok · Playwright 11/11 · eslint + clippy clean.
Live smoke test (04:20): today's league via `dump_state` → the new prompt shape
(5.4k chars of state JSON + 20k chars of board table, 419 rows) → real `claude`
CLI with the app's flags. Opus answered a QB2/DEF plan naming Purdy (ADP 122),
Rams / Philadelphia / Denver / Detroit DEF — players far outside the old top-40
slice — in 36 s, ~42k context tokens, $0.50 as the CLI prices it. Timeouts were
raised to 90 s (150 s with web search) on the strength of that measurement.

Replay run (05:35–06:05): `bun run verify` re-run after R-1/R-2 — Vitest 33/33,
Rust 56 + suites, Playwright 11/11, lint/clippy/fmt clean.

To use your real logged-in Chrome for a Sleeper mock draft next time, either:
`open -na "Google Chrome" --args --user-data-dir=$HOME/chrome-debug --remote-debugging-port=9222`
(log the testing account in there once), or save its credentials with
`agent-browser auth save sleeper --url https://sleeper.com/login --username <name> --password-stdin`.
Chrome ≥ 136 refuses remote debugging on the default profile, so the plain
"already logged in" window cannot be attached to.

Nothing committed yet — all of the above is in the working tree of
`t3code/review-prior-grade-report`.

## Why this beats a crib sheet / the Sleeper web UI (summary)

- **A crib sheet is frozen at print time.** It cannot know your league's exact
  scoring (6-pt pass TD, PPR, DEF buckets), cannot re-rank as players leave the
  board, and cannot tell you "who survives to pick 55". The app's board is
  scored from the league's own `scoring_settings`, VORP is measured against
  real replacement demand for a 14-team roster shape, and survival odds are
  recomputed from the live pick count every 3 s. A crib sheet is still worth
  having as a sanity check for a name the model or the projections mis-price.
- **Sleeper's own UI shows ADP and default ranks,** not value over
  replacement, not tier scarcity, not your open starters, not who is likely
  gone before you pick again. It also cannot be asked a question.
- **What the AI actually adds** is judgement over the numbers, not the
  numbers: trade-offs ("RB2 now or the last tier-2 TE?"), roster construction
  across the next three picks, why a flagged player matters (with web search
  on), and explaining the app's own recommendation in plain words. It is fed
  the same state the board renders, so it cannot invent a player; the
  deterministic recommendation is the floor, the chat is the second opinion.
