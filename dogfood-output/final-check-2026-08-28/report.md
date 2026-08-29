# Pre-draft refresh and full functionality check — 2026-08-28, 09:25–09:35 PDT

Commit `726c54c` · draft in ~7.5 hours · desktop app running against the real
league the whole time.

## The refresh

The app's own caches (`~/Library/Application Support/com.justin.draft-assistant/`)
were moved aside and the desktop app relaunched, so all three were re-downloaded
from scratch rather than partially refreshed.

| File | Before | After | Note |
|---|---|---|---|
| `players.json` | 03:13 | **09:27** | 12,225 players both times |
| `projections_2026.json` | 03:13 | **09:27** | 3,303 rows |
| `weekly_2026.json` | 03:13 | **09:27** | 18 MB |

What actually changed in six hours: **one** injury tag — Cameron Dicker → Questionable
(a kicker; this league has no K slot). Nothing else moved. The board, the
recommendation and every derived number are identical to the 08:35 dump.

Cold rebuild from an empty cache took **6.5 s** end to end, including both
multi-megabyte downloads under their new 60-second caps.

## Functionality checked, on tonight's real data

| Area | Result |
|---|---|
| **Desktop app** | Launched, no panic, rewrote `config.json` at 09:29 (full React → IPC → engine → Sleeper round trip), 45 established sockets while polling |
| **Pre-draft banner** | "ROUND 1 · PICK 1 · **Draft has not started · starts 5:00 PM** · YOUR PICKS 2 · 27 · 30 · 55" — the keeper fix and the start time both holding on live data |
| **Live region** | Announces the status line only ("Draft has not started · starts 5:00 PM"), not the board or a ticking clock |
| **Keepers** | Kenny Gainwell R10 and Matthew Stafford R14 on your roster; 139 and 195 absent from your remaining picks; open starters recomputed to WR×1, TE×1, FLEX×4, DEF×1; 394 of 419 players available |
| **Recommendation** | Jahmyr Gibbs — 353 pts, VORP 196, Tier 1, survives 74 %, reasons: top VORP, fills an open FLEX slot, thin at RB |
| **Tier alerts** | "QB Top tier T2 · 4 left", "RB Top tier T1 · 2 left" … with the explanatory tooltip |
| **Recent picks** | "None yet." — keepers correctly excluded before the draft reaches them |
| **Board** | 200 of 394 shown; **Show all** → 394 and back; `/` focuses search ("gibbs" → 1 row); RB filter → 90 rows; sort by Surv descending flips `aria-sort` |
| **Confirm dialog** | "Mark Jahmyr Gibbs as drafted at pick 1 (slot 1)?", focus lands on Confirm, Escape closes it |
| **Ask Claude** | Real end-to-end CLI call against tonight's board: 42,909 tokens of context, 3.8 s, $0.24. The answer named Gibbs at pick 2, cited VORP 196, knew both keepers, and identified WR as the hole given four FLEX slots — see below |
| **Live sync** | Replay stepped 55 → 57: recent picks, board and roster followed within one poll; pill green; "Last sync 4s ago" counting; pick clock ticking and resetting on each new pick |
| **Undo** | Disabled — no manual picks exist |
| **Warnings / console** | None, on any screen |
| **Test suite** | `bun run verify` exit 0: 108 Rust, 61 Vitest, 15 Playwright, clippy, eslint, fmt, LOC cap |

The chat's answer, verbatim:

> Take Jahmyr Gibbs at 2 — he's the board's top VORP (196) and the last elite
> RB1 you'll see, since Bijan or Chase will be gone by your 27/30 turn. Your
> biggest hole is WR: you have zero, and with four FLEX slots plus a starting
> WR you need volume there, so plan on WRs back-to-back at 27/30.

## One thing to know about the refresh

Projections carry a **6-hour** TTL. Refreshed at 09:27, they expire at **15:27** —
still before the 17:00 draft. That does not matter while the app stays open (the
board lives in memory), but a *relaunch* after 15:27 re-downloads ~20 MB.

- Leaving the app running from now: nothing re-downloads.
- Relaunching for the draft: refresh once more after ~11:00 to cover a 16:45
  launch, or just let it re-fetch — with the new 60-second caps a cold rebuild
  is 6.5 s on a decent connection, and a failure falls back to the cached copy
  with a banner.
