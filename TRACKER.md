# Work tracker — 2026-08-28 (draft day, 17:00 PDT)

Status legend: `todo` · `doing` · `done <commit>` · `deferred (why)`

Updated as work lands. Items come from the grade report (`.claude/grade-report.md`),
the dogfood report (`dogfood-output/report.md`), and the in-app Claude's own list of
what it could not see.

## Requested this session

| # | Item | Status | Notes |
|---|------|--------|-------|
| 1 | B6 — chat memory + configurable model | done 86c42e3 | `chat/prompt.rs` sends the last 6 exchanges + any summary; `DRAFT_ASSISTANT_CLAUDE_MODEL` default; stub-CLI tests assert history and flags |
| 2 | D2 — unit-test command-layer pure logic | done 86c42e3 | `manual.rs` (`apply_manual_pick` / `undo_manual_pick`, 5 tests, plus a new not-on-the-board guard); `sleeper::extract_id` (5 tests) |
| 3 | Chat: model selector (Opus / Sonnet / Fable) | done 86c42e3 | Settings fold in the panel; remembered in localStorage |
| 4 | Chat: speed selector (fast mode) | done 86c42e3 | `--settings '{"fastMode":true}'`; the account reports `extra_usage_disabled`, so the panel notes once "answered at standard speed" — enable extra usage to get it |
| 5 | Chat: thinking/effort selector | done 86c42e3 | `--effort low/medium/high/xhigh/max`, Default = CLI default |
| 6 | Chat: full app context (whole board, not top 40) | done 86c42e3 | all 419 players as a pipe table (~10k tokens vs 34k for JSON); state JSON keeps rosters, recent picks, baselines |
| 7 | Chat: web-search toggle, default off | done 86c42e3 | `--tools WebSearch` when on, `--tools ""` when off; system prompt tells the model when it may search |
| 8 | Chat: new-session button | done 86c42e3 | **New chat** in the panel header |
| 9 | Chat: token usage display | done 86c42e3 | usage line: context tokens, seconds, model, question count, session cost, web searches |
| 10 | Chat: compaction option with "takes a minute or two" warning | done 86c42e3 | **Compact** (enabled after 2 questions); warning in the tooltip and the pending line; summary replaces the thread as history |
| 11 | Board: click column headers to sort (Bye, Pos, Pts, VORP, Tier, ADP, Surv) | done 86c42e3 | every header sorts; second click flips; `#` restores value order; blanks last; `aria-sort` set; unit + E2E tests |
| 12 | Why `weekly_2026.json could not be cached (replace …: No such file)` | done 86c42e3 | Two loads overlapped (React dev StrictMode double-mount at launch) and shared one `weekly_2026.json.tmp`; the loser's rename found nothing. Fix: per-write tmp names (`store.rs`, 8-thread race test) + launch effect guarded to run once (`App.tsx`). Harmless in effect: the data was in memory and used. |
| 13 | Claude's wish list: full board / weekly data / live picks | done / deferred | full board: done (item 6); weekly cache: done (item 12); schedule/SoS/playoff weeks: deferred (schema change); live picks: already flow in |
| 14 | Dogfood ISSUE-001 — confirm modal ignores Escape, no focus | done 86c42e3 | native `<dialog>` (`ConfirmDialog.tsx`): focus lands on Confirm, Escape/backdrop cancel, focus returns to the row; E2E test |
| 15 | Dogfood ISSUE-002 — chat Escape lost after focus leaves | done 295d2c9 | |
| 16 | Dogfood ISSUE-003 — chat drawer covers header + decision columns | done 86c42e3 | panel is a sticky flex column beside the page; board scrolls horizontally if needed; E2E asserts nothing actionable sits under it |
| 17 | Dogfood ISSUE-004 — sticky alert covers a board row | done 86c42e3 | toast anchored top-right under the header |
| 18 | Dogfood ISSUE-005 — header wraps at 1000 px | done 86c42e3 | league name ellipsises, buttons wrap as whole units, pill never breaks mid-word |
| 19 | Dogfood ISSUE-006 — preseason QUESTIONABLE penalised in safe mode | done 86c42e3 | `recommend::serious_injury` (out/ir/pup/sus/doubtful/na/cov) gates the −15; badge muted for Questionable with a tooltip; tests both sides |
| 20 | Dogfood ISSUE-007 — preview greets with red alert | done 86c42e3 | `api.preview` skips live sync and shows a neutral banner; E2E asserts no alert on load |
| 21 | Write-up: crib sheet vs app, what AI adds, app vs Sleeper web | done | see the section at the bottom |
| 22 | Demo / replay mode (asked 05:25) | done 86c42e3 | `scripts/replay-sleeper.mjs` replays any completed draft as live; `DRAFT_ASSISTANT_SLEEPER_BASE` override in `sleeper.rs` (D1's seam); browser preview `?replay=<url>` polls dumps; README "Demo / replay mode" |
| 23 | Live test of the app via Chrome + mock draft | done differently | your logged-in Chrome is unreachable from this harness (no debug port; Chrome profile folder blocked by macOS). Tested instead against a live replay of the league's 2025 draft with you at slot 2 — 13 scenarios, all pass: `dogfood-output/replay/report.md` |
| 24 | R-1 sticky alert covered wrapped header buttons (regression from item 17) | done 86c42e3 | found by the replay test; failures are now an in-flow bar under the header, toasts are click-through |
| 25 | R-2 recommendation card shown after draft complete | done 86c42e3 | `build_view` returns no recommendations once `draft_over` |
| 26 | Bug hunt (asked 06:00) — 12 validated items H1–H12 | done, all 12 | each item: failing test → fix → passing → own commit. H1 3c83f8c · H2 7115c33 · H8 a6f5e98 · H9 e5d53c7 · H6 693383b · H7 ec52755 · H3 5cefda4 + 6901d95 · H4 d76d6a5 · H5 d2d8359 · H10 9f9dccb · H11 a257c0f · H12 c6c9731. Results table in `dogfood-output/bug-hunt-2026-08-28.md` |
| 27 | Replay server stamps `last_picked` at real release time | done | so the new pick clock counts down in replay mode like a live draft |
| 29 | Dogfood pass, all areas (asked 07:00) | done, 13 issues | `dogfood-output/full-2026-08-28/report.md` — 1 high (live-sync false success), 7 medium (false success toasts ×2, silent launch failure, raw parser error, clock live-region chatter, board clipped with chat open), 5 low. Verified-working table covers sorting/search/dialog/clock/live sync/chat. Desktop-only paths untestable (WKWebView has no CDP) |
| 30 | Fix all 13 dogfood issues (asked 07:05) | done, all 13 | test-first per item: 001–005+011 `5c7d672` · 008 `ec1ff0a` · 006/010/012 `6fffbb4` · 009 `d0c54eb` · 007 `6232c9d` · 013 `fdbdbfa`. Results table in `dogfood-output/full-2026-08-28/report.md`, each verified in the running app |
| 31 | **Keeper league support** (found while regenerating the fixture) | done | the 2026 draft carries 23 keepers at picks 11–177 while still `pre_draft`: the clock read "Pick 24" before anyone picked, manual picks were silently discarded, and recent picks led with round-13 keepers. `aedfa8f` + `467ba44`, covered by `src-tauri/tests/keepers.rs` |
| 32 | Dogfood pass 2 (asked 08:20) | done, 4 issues, all fixed | `dogfood-output/pass2-2026-08-28/report.md` — 1 high (keepers made the app claim the draft was live 10 h early), 3 low (tier badge colours, dead `injured` class, board clipped at 1100px). All fixed test-first in `f5c3a1a`. Clean: error boundary, corrupt settings, interaction races, 5-min soak (flat 16 MB, 0 errors), tonight's real state rendered end to end |
| 33 | Pre-draft shortlist from the grade report (asked 09:15) | done | B1 large-download timeouts `f1339b0` · C5 `/` to search + C2 "Show all" `82068f8` · H1 pre-draft runbook in the README. Re-dumped tonight's league after the changes: byte-identical state, no warnings |
| 34 | Push + PR so CI finally runs (grade items D3/I1) | done, green | [PR #1](https://github.com/jtn0123/draft-assistant/pull/1). It failed three times first, each a real defect only CI could find: start-time tests asserted a Pacific wall-clock string (`424a09f`), the runner's Rust 1.98 flagged three lints my 1.88 does not — now pinned via `rust-toolchain.toml` (`ca3dd84`) — and two App tests rendering 200 board rows blew the 5 s timeout on a loaded runner (`bb48e47`). Run 33188132961 green |
| 35 | Pre-draft data refresh + full functionality check (asked 09:25) | done | All three app caches re-downloaded 09:27 (one injury tag changed in six hours: Cameron Dicker, a kicker). Everything exercised on tonight's real data — banner, keepers, board, dialog, live sync, a real $0.24 Claude call, `verify` exit 0. `dogfood-output/final-check-2026-08-28/report.md`. Note: projections TTL is 6 h, so a relaunch after 15:27 re-downloads ~20 MB |
| 36 | AI integration: fix all six gaps from the assessment (asked 10:39) | done | **Streaming** — `stream-json` parsed line by line (`chat/stream.rs`), text pushed over a Tauri `Channel`, the panel renders the answer as it is written with a caret, Cancel keeps what was written. **Markdown** — `chatMarkdown.tsx` renders bold/italic/code, bullet and numbered lists, headings; no HTML ever interpreted. **Cost** — the user's own MCP servers were adding ~16k tokens of tool schemas to every call; `--strict-mcp-config` drops them (42.9k → 21k context tokens on the real board); a session budget ($5 default, 0 = none) stops asking when reached. **Staleness** — every answer is stamped `as of pick N`, amber with "k picks since" once picks land. **Injection** — state and board fenced in `<draft_state>`/`<board>` tags the system prompt names as data; names stripped of pipes/newlines and clipped. **Proactive** — "Ask when I'm on the clock" fires the one question that matters the moment it is the user's turn, once per pick, and opens the panel. Real three-question session recorded through the new path with `dump_state --ask … --chat-out` (5.7 s / 7.7 s / 19.5 s, $0.67) and screenshotted via `?chat=` replay: `dogfood-output/ai-session-2026-08-28/`. 24 new tests (9 Rust, 11 Vitest, 1 E2E + Markdown suite) |
| 37 | Heavy test building toward 80 % coverage (asked 11:50) | done | Measured first: Rust **72.1 %** lines, frontend **86.2 %**. Extracted the Tauri command bodies and the poll loop into a Tauri-free `app::AppCore` (grade A1/D1 — `desktop.rs` is now 150 lines of glue), built a stub Sleeper server + stub Claude CLI shared by the integration tests (`tests/support/`), and added 25 Rust tests (engine fetch/cache/fallback matrix, every command incl. manual-pick rollback, the poll state machine, `dump_state` run as a real process) plus 8 for `api.ts` (Tauri channel bridge, replay polling, recording playback). Result: Rust **92.0 %** lines (`desktop.rs`/`main.rs` are the only zeros), frontend **93.4 %**, `api.ts` 100 %. `bun run coverage` reproduces both numbers. `dump_state` gained `DRAFT_ASSISTANT_DATA_DIR` so tests never touch the real CLI cache |
| 28 | Full `bun run verify` after the bug-hunt fixes | done, exit 0 | Rust 98 tests (69 lib + 29 integration), Vitest 41, Playwright 11, clippy/eslint/LOC clean; replay preview screenshot `dogfood-output/replay/13-bug-hunt-fixes-pick-58.png` |

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
| 38 | Saved chat sessions + new sessions (asked 12:55) | done | One JSON file per session under the data dir (`chats/<draft_id>/<id>.json`, `chat/session.rs`, tmp+rename, ids sanitised to one path segment), three commands (`save/list/load_chat_session`) forwarded through `AppCore`, a **Sessions** row in the panel that lists the draft's sessions and reopens one in place, autosave after every answer/compaction/cancel, restore of the latest session when a draft opens, and **New chat** starting a separate file. Browser preview runs the same over `localStorage` (`sessionStore.ts`). Tests: 4 Rust (`tests/chat_sessions.rs`) + 2 unit, 5 Vitest (`ChatSessions.test.tsx`) + 2 (`sessionStore.test.ts`), 1 Playwright (reload restores, New chat separates). The e2e caught a real bug: a "restored already" ref made StrictMode's double mount cancel the restore for good. |
| 39 | Dev watcher reloads the live window on test output (I2, observed 12:37) | done | `vite.config.ts` `watch.ignored` now covers `coverage/`, `playwright-report/`, `test-results/`, `dogfood-output/` and `public/ai-*.json`. Takes effect on the next `tauri dev` launch. |
| 40 | `verify` fails after a coverage run (I1) | done | `eslint.config.js` ignores `coverage`, `playwright-report`, `test-results`, `dogfood-output`; `scripts/check-loc.mjs` skips `coverage/` too (it had the same blind spot — three 500+-line HTML reports). |
| 41 | Keeper tags, keyed on position not the flag (B2) | done | `Pick.is_keeper` deserialised as a hint; `view::keeper_pick_nos` judges keepers by "in the book at or beyond the next open pick"; `LoadedLeague.keeper_pick_nos` remembers them across polls so a keeper stays one after the draft passes its slot; `RosterEntry.is_keeper` rendered as a **keeper** tag; keepers excluded from recent picks and runs whether or not the draft has reached them. Live feed at 12:41: 27 keepers, 24 flagged. Tests: 2 in `keepers.rs`, 1 in `app_core.rs`, 1 Vitest. |
| 42 | Weekly-projection path untested (D2) | done | `tests/board_fixture.rs`: bye inference (full slate, a stray traded-player row, a team with no weekly rows), per-game bonus expectation only when the league pays it, and the "no scored players" degradation. `board.rs` 84.6 % → 94.6 % lines. |
| 43 | Honest frontend coverage (D1) | done | `vitest.config.ts` `coverage.include` names every source file; `api.ts` appears (94.5 %), the headline is 91.4 % over all files rather than 93 % over a subset. `Markdown.tsx` still does not appear in the v8 table despite the include — noted, not chased. |
| 44 | Drive the real desktop window and test it (asked 15:50) | done | Accessibility + screen recording granted, so the real WKWebView window was driven with CGEvent clicks and screenshotted. Verified live: keeper tags on the roster, session restore on open, a question asked **through the panel** (streaming Channel → answer → autosave), New chat writing a second file, the Sessions switcher, `/` search, position filter, ADP sort, Export state, and manual pick → Undo. Found and fixed two real bugs (row 45) plus a cosmetic one: a reopened session's label read "not saved yet". Screenshots in `/tmp/app-test/`. |
| 45 | **Manual picks were silently deleted 3 s after being made** (found 16:05 by driving the app) | done | `engine::reconcile_manual_picks` still used the old `pick_no > api_max` rule that `view::merged_picks` was fixed away from. This league's feed opens with keepers up to pick **195**, so *every* manual pick made tonight was retired by the next poll — the fallback for a lagging Sleeper API was completely broken, and it failed silently. Second half: the poll only emits a view when the *feed* fingerprint changes, so the UI kept rendering the phantom pick and **Undo** answered "no manual picks to undo". Fixed both (keyed on pick number + player, and `manual_changed` forces an emit); 1 unit + 2 integration regression tests; re-verified in the real window — the pick now survives 4+ polls and Undo restores pick 1 cleanly. |
