# Draft-app

A local-first fantasy football assistant for Sleeper leagues, built with
Tauri 2 (Rust core + React/TypeScript frontend). Read-only: it polls the public
Sleeper API and never writes anything back.

**The app lives in [`draft-assistant/`](draft-assistant/README.md)** — start
there for what it does, how it works, and the module layout.

## Quick start

```bash
cd draft-assistant
npm install
npm run tauri dev
```

To see the UI without building the Rust side, `npm run dev` serves it at
:1420 against the committed fixtures in `public/`.

## What else is here

| Path | What it is |
|------|-----------|
| `draft-assistant/` | the application |
| `TOP15.md` | the fix list currently being worked through |
| `TRACKER.md` | running log of what has landed, newest at the bottom |
| `.claude/grade-report.md` | full codebase audit: 9 categories, numbered items |
| `research/` | scratch notes, gitignored |

## Conventions

- **Every file is 500 lines or fewer**, enforced by `scripts/check-loc.mjs`.
- **`npm run verify` is the gate** — LOC cap, rustfmt, tsc, vite build, both
  test suites, eslint `--max-warnings=0`, clippy `-D warnings`. CI runs exactly
  this command, so local and CI cannot drift.
- **Warnings get fixed, never suppressed.** No `-A dead_code` and friends.
- Regenerate `public/dev-fixture.json` and `public/dev-season-fixture.json`
  together, or the browser preview shows a fresh draft beside a stale season.
