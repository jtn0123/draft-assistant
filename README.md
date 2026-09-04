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

## Installing on macOS

CI builds a `.dmg` on every push to `main` (the **Bundle macOS app** job's
`draft-assistant-macos` artifact). It is **ad-hoc signed and not notarized** —
there is no Apple developer account behind this project — so Gatekeeper will
refuse to open it with "Draft Assistant is damaged and can't be opened."
Nothing is damaged; that is the message macOS shows for a download whose
quarantine flag it cannot check a signature against.

Drag the app out of the `.dmg` to `/Applications`, then strip the flag:

```bash
xattr -d com.apple.quarantine /Applications/Draft\ Assistant.app
```

It opens normally after that, and stays open — the flag is only set on
download. Building it yourself with `npm run tauri build` skips this entirely,
because a locally built app is never quarantined.

## What else is here

| Path | What it is |
|------|-----------|
| `draft-assistant/` | the application |
| `TOP15.md` | the fix list currently being worked through |
| `TRACKER.md` | running log of what has landed, newest at the bottom |
| `.claude/grade-report.md` | full codebase audit: 9 categories, numbered items |
| `research/` | scratch notes, gitignored |
| `LICENSE` | MIT |
| `SECURITY.md` | how to report a vulnerability |

## Phone & second screen

The desktop app can host a small HTTP + WebSocket server on the LAN so a phone
(a trimmed page) or a second copy of the app (full UI, follower mode) can watch
the same league and share one chat thread. Off by default, port 7878, and the
whole contract lives in [`COMPANION-API.md`](COMPANION-API.md).

Turn it on in Settings → **Phone & second screen**; join one from Settings →
**Join another Draft Assistant…** (also on the first-launch screen). A follower
is read-only by construction: the league, the API key, the budget, Yahoo and
the username all stay with the host, and `src/apiRemote.ts` refuses those calls
by name rather than half-doing them.

**Why `connect-src` in `tauri.conf.json` allows `http:` and `ws:`.** A follower
talks to whatever address the user typed — any LAN IP, and any port the host
picked when 7878 was taken. There is no way to enumerate that in a CSP: it
would mean listing `http://*:7878` and the next ten ports, and still failing
for the eleventh. So `connect-src` alone is widened to `http: ws:`, and every
other directive stays exactly as strict as it was — `default-src 'self'`,
`img-src` still limited to `self`, `data:`, `asset:` and Sleeper's CDN, no
`script-src` relaxation at all. The app never navigates to a remote origin and
never loads remote code; it only fetches JSON and images from a host the user
paired with by typing a six-digit code.

## Conventions

- **Every file is 500 lines or fewer**, enforced by `scripts/check-loc.mjs`.
- **`npm run verify` is the gate** — LOC cap, rustfmt, tsc, vite build, both
  test suites, eslint `--max-warnings=0`, clippy `-D warnings`. CI runs exactly
  this command, so local and CI cannot drift.
- **Warnings get fixed, never suppressed.** No `-A dead_code` and friends.
- Regenerate `public/dev-fixture.json` and `public/dev-season-fixture.json`
  together, or the browser preview shows a fresh draft beside a stale season.

## License

MIT — see [`LICENSE`](LICENSE).
