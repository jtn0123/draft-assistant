# Live replay test — 2026-08-28 05:35–06:05 PDT

**Setup.** Your logged-in Chrome could not be reached from this harness (no
debugging port; macOS blocks the Chrome profile folder), so instead of a
Sleeper mock draft the app was tested against a **replay of your league's own
2025 draft** (`1236044119889940480`: 14 teams × 15 rounds, 210 picks) served
by the new `scripts/replay-sleeper.mjs` as if it were happening live — one
pick every 8 s, you placed at **slot 2** exactly as tonight, season rewritten
to 2026 so today's projections and caches were used. The engine
(`dump_state`, same code as the desktop backend) rebuilt the state every ~5 s
and the real UI followed it in Chrome via the browser preview's new
`?replay=` switch.

Not covered: the Tauri window itself (no screenshot access from here). Its
poll loop and event emit were verified earlier today by side effects; every
other layer — Sleeper client, engine, view, React UI — ran for real here.

## Scenarios and results

| # | Scenario | Result |
|---|----------|--------|
| 1 | Draft goes `pre_draft` → `drafting` as the first pick lands | ✅ status flipped, clock started counting |
| 2 | Live sync pill and "Last sync Ns ago" while dumps arrive | ✅ "● Live sync on · Last sync 1s ago" throughout |
| 3 | Round / pick / on-the-clock name / "N picks until you" advance | ✅ matched the server log pick for pick |
| 4 | Recent picks list matches the server's release log | ✅ e.g. "24. Jaxon Smith-Njigba · WR · slot 5" |
| 5 | Board shrinks as players go (419 → 395 → 365 → 348) | ✅ |
| 6 | Your roster fills from slot-2 picks (Bijan, K. Walker, Sutton, G. Wilson…) | ✅ |
| 7 | **YOU ARE ON THE CLOCK** at your picks (55, 58) with `clock mine` styling, next picks 27·30·55·58 | ✅ screenshot `07-on-the-clock-58.png` |
| 8 | Recommendations, tier alerts, "🔥 WR run in progress", open-starter line update live | ✅ |
| 9 | **Confirm dialog open on the player who is about to be picked** (focus on Confirm) → server releases that pick | ✅ dialog closed itself, alert "Garrett Wilson was drafted by another team — pick cancelled", he appeared on your roster, clock moved to pick 56 |
| 10 | Column sort (Bye ▲) survives three live updates | ✅ rows still ordered, `aria-sort` kept, blanks last |
| 11 | Ask Claude panel in the preview: opens with focus, settings line, suggestion → clear "run the desktop app" error, Escape closes | ✅ |
| 12 | Draft completes at 210 picks | ✅ "Draft complete", roster 15/15, no recommendation card (fixed during the test, see below) |
| 13 | Rewinding the replay (`/replay/set?n=60`) is absorbed by the UI | ✅ state simply followed the newer dump |

Evidence: `screenshots/06`–`12`, `videos/replay-live-45s.webm` (45 s of live
play at 8 s/pick), `replay-server-log-head.txt` (every poll the engine made),
`upcoming.txt` (the recording's pick order used to target scenarios 9 and 12).

## Found and fixed during the test

**R-1 — Sticky failure toast covered the header buttons (regression from this
morning's ISSUE-004 fix).** After scenario 9 the "pick cancelled" alert sat at
the top-right, and at 1280 px the header actions wrap to a second row; the
alert landed exactly over **Ask Claude** — `elementFromPoint` at the button's
centre returned the alert, and clicks on it were swallowed. The desktop
window would hit the same thing whenever its header wraps (1000 px minimum).
Fix: sticky failures are now an in-flow red bar under the header (nothing can
be covered); transient confirmations stay a corner toast but are
`pointer-events: none`. Re-verified live: bar top 118 px vs header bottom
104 px, zero header buttons overlapping (`12-alert-bar-after-live-pick.png`).

**R-2 — A recommendation card after the draft was complete.** At 210/210 the
cards still named Harold Fannin. `build_view` now returns no recommendations
once `draft_over`.

## Worth knowing (not bugs)

- The replay pairs **2025 picks with the 2026 board**, so players the 2025
  drafters ignored sit on the board far past their ADP — Puka Nacua (rank 3,
  ADP 4) was still there at pick 58 showing "survives 1%". That is the engine
  reading the situation correctly; it just will not happen with tonight's real
  picks.

## Limitations noted

- agent-browser's `screenshot`/`record stop` hung in every session this run
  (evals and clicks worked); visuals were captured with Playwright instead.
- The preview lags the server by 3–9 s (dump every 4 s + poll every 3 s); the
  desktop app polls Sleeper directly every 3 s and has no dump step.
