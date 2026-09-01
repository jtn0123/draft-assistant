# Replay: a preview that moves, and the browser suite that watches it

The browser preview (see [Browser preview](../README.md#browser-preview))
normally renders two captured dumps, read once. Replay mode makes it move.

## Pointing the preview at a moving source

`?replay=<url>` and `?replay-season=<url>` point either dump somewhere else and
turn the preview live: that source is re-read every three seconds, and every
dump with a newer `generated_at` is pushed through the same listeners the
desktop poller feeds — `draft-updated`/`poll-health` and their season twins, so
the screens cannot tell replay from the real thing. Ordering is by
`generated_at` rather than a sequence number because each `dump_state` run
numbers itself from scratch; an older dump is ignored rather than rewinding the
board, and a dump caught half-written is skipped until the next tick reads it
whole.

Without those parameters nothing changes: two fixtures, read once, and live
sync says it needs the desktop app. The logic is `src/replay.ts`; `src/api.ts`
owns the two feeds.

A wrong path is the common mistake, and the dev server answers one with 200 and
`index.html` — so the preview says the source is not a state dump, rather than
passing on `Unexpected token '<'`.

## The replay server

`scripts/replay-sleeper.mjs` writes a source worth polling. It stands in for
api.sleeper.app on a local port: one recorded draft's league, draft, picks, and
traded picks are served from a recording with the picks released on a timer,
and everything else (players, projections, users, scores) is proxied upstream.

```bash
cargo build --manifest-path src-tauri/Cargo.toml --bin dump_state --bin dump_season
npm run replay -- --league <id> --draft <id> --interval 8 \
  --username <name> --dump public/live-state.json
npm run dev   # then open http://localhost:1420/?replay=/live-state.json
```

`--dump-season public/live-season.json` does the same for the season screen,
read back with `?replay-season=/live-season.json`. Run the script with no
arguments for the full option list.

Control it while it runs, on the same port:

| Request | Does |
|---|---|
| `GET /replay/status` | how many picks are out, and whether the clock is paused |
| `GET /replay/step` | release one more pick |
| `GET /replay/pause`, `/replay/resume` | stop and start the clock |
| `GET /replay/set?n=<picks>` | jump to a point in the draft |

## Driving the desktop app from it

Set `DRAFT_ASSISTANT_SLEEPER_BASE=http://localhost:8787` and every Sleeper URL
is rewritten to the replay server (`src-tauri/src/sleeper_host.rs`). **Only a
debug build honours it** — a release build ignores the variable entirely, so a
shipped app can never be pointed at another host. That is also why the dump
binaries above are built in debug.

## The browser end-to-end suite

`npm run test:e2e:browser` runs the Playwright specs in `e2e-browser/` against
the preview: a real Chromium, a real layout engine, and the checked-in dumps.
It covers what jsdom cannot — overflow at the 1000px minimum width, the roving
tabindex on the season rail, and the replay poll actually moving the screen
without a reload — and deliberately does not touch the Tauri IPC boundary,
which the preview stubs out. `npm run test:e2e` is the separate, much heavier
WebdriverIO suite that drives the real desktop window.

`@playwright/test` is a devDependency; the browser it drives is installed
separately:

```bash
npx playwright install chromium
npm run test:e2e:browser
```

That download is why this is not part of `npm run verify`.
