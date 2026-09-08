# Testing

Split out of `README.md` to keep both documents within the 500-line cap the
repo enforces on every first-party file.

`npm run verify` is the gate: format, lint, typecheck, the vitest suite, the
Rust suite with coverage floors, guard-script tests, and a production `vite build`.
It does not require live Sleeper or provider requests; Rust compilation and
coverage can take several minutes, especially on a cold cache.

CI (`.github/workflows/ci.yml`) runs that plus the things that need the
network or a browser: `npm run check:version`, `npm run test:scripts` (the
node tests beside the guard scripts in `scripts/`), `npm run audit` (npm's
advisories in both trees through `scripts/check-npm-audit.mjs`, cargo's
through `cargo audit`; the two known-unfixable ones are allow-listed by id in
`scripts/npm-audit-allowlist.json` and `src-tauri/.cargo/audit.toml` with the
reason beside each), and the Playwright suite against the production bundle
(`npm run test:e2e:browser:ci`: `vite build` then `vite preview` on port
1420, where `npm run test:e2e:browser` drives the dev server locally). The
release workflow calls this one and waits on it, so a tag gets the same gate.

The draft preview fixture uses schema 1.7, including `draft_projections`.
It contains roster identities and picks but no projections for drafted players;
its available-player list excludes them. Projection rows therefore match the
engine's missing-data behavior: zero points, every starter slot open, and stable
roster-order ranks. With zero means and spreads, the current simulation breaks
all ties toward the first roster (100% versus 0%); those fixture odds are not
football estimates. Real loaded boards retain drafted-player projections.
`fixture_shape` checks the complete serialized shape without exceptions for this field.

Playwright uses zero retries and retains traces and screenshots on failure.
Both local and CI runs generate `playwright-report/`. If the browser step fails,
CI uploads that HTML report plus `e2e-browser/.results/` (including the hidden
results directory) as `playwright-report`. Inspect it with
`npx playwright show-report`, or open a trace with `npx playwright show-trace <trace.zip>`.

Vitest's coverage floors count every file under `src/` (`coverage.include`),
not only the files some test imports, so removing a screen's tests lowers the
number instead of raising it. Its per-test timeout is 20 s: the longest
`waitFor` budget in `App.test.tsx` is 5 s, which the old 5 s default made
unreachable under load.

### What is not covered

Nothing in `verify` launches the app. The React bundle is tested in jsdom, the
Rust is tested by calling functions directly, and the two never meet — so the
seam where they do meet in production is the one thing no test watches: the
command names the frontend types into `invoke()`, the capability set, and the
CSP the built bundle has to load under.

`src-tauri/tests/command_surface.rs` closes the first of those three. It stands
the app up on Tauri's mock runtime with the same state `lib.rs` installs, sends
each command a real IPC message, and fails if the dispatcher does not recognise
the name. It also reads `lib.rs` and asserts that the `generate_handler!` list
is exactly the set of `#[tauri::command]` functions in the crate, which is the
one failure that otherwise reaches the user: a command written, wired up in
`api.ts`, and never registered. Two commands take a bare `tauri::AppHandle`
(i.e. `AppHandle<Wry>`) and so cannot be registered on the mock runtime;
they are covered by the source-level check but not the IPC round trip.

The capability set and the CSP are covered by `npm run test:e2e` below, a real
window rather than a mock — not in `verify`, so between runs the manual check
stands in. `npm run test:e2e:browser` drives the preview: [docs/replay.md](docs/replay.md).

### Manual smoke check

Sixty seconds, and it sees more than the automated run does. Do this after
touching `tauri.conf.json`, `capabilities/`, `lib.rs`, or the Vite chunking;
`npm run test:e2e` covers item 1 and half of item 2, and nothing else here:

```bash
npm run tauri dev
```

1. The window opens and is not blank. A blank window with content in the DOM
   means the CSP rejected a chunk — check the WKWebView console.
2. The saved league restores, or the setup screen offers to add one. Either way
   the launch screen resolves; if it hangs, `get_config` did not answer.
3. The draft board renders rows, and picking a player opens the confirm dialog.
4. The season screen opens and shows the matchup, lineup and standings.
5. Player headshots and manager avatars render. They arrive as `data:` URLs, so
   a missing image is usually a change to `img-src` in the CSP.
6. Ask AI opens and reports a provider.

### Native draft rehearsal

From `draft-assistant/` on macOS, run:

```bash
npm run test:e2e:rehearsal
```

This builds and launches the native WKWebView app with an isolated rehearsal
profile and a local Sleeper fixture. It checks initial loading, incoming API
picks, manual pick recording and undo, then a simulated outage and recovery.
Your saved league/profile is separate from the rehearsal data.

The runner prints a temporary evidence directory (`draft-native-rehearsal-*`).
Screenshots, build/install/test logs, fixture requests, and rehearsal data remain
there after the run. The temporary profile link is removed when the runner exits
normally, including test failures. This checks native behavior deterministically;
verify the actual league, draft order, and start time against live Sleeper too.

### Phone page as a phone

```bash
npm run test:e2e:mobile
```

Walks the companion page in WebKit as an iPhone 15 and in Chromium as a Pixel 7, light and dark: pairing, the your-turn nudge with the urgent clock, the best-available list, the chat with an answer in flight, and a dropped socket brought back by tapping the pill. No dev server; `e2e-browser/companionServer.ts` serves the shipped files and answers as the host. Screenshots land in `e2e-browser/.results-mobile/`. Team marks are let through to Sleeper's CDN so the pictures in the screenshots are real, and the first one is asserted to have loaded; headshots go through the fake host, which has none. Service workers are blocked in that config on purpose: Chromium treats localhost as a secure context, registers the page's worker, and the worker's fetches go around Playwright's routing to whatever is really on the port. This is browser emulation, not a phone: sound, vibration and the iPhone keyboard are only proven on hardware.

### Phone and desktop with a real companion host

From `draft-assistant/`, with npm dependencies and Playwright Chromium installed
and localhost port 1420 free, run the isolated browser rehearsal:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test companion_wire phone_and_desktop_follow_a_real_host_and_recover_together -- --ignored --nocapture
```

The Rust driver starts a fixture companion host and supplies its temporary
pairing credentials to the browser helper; do not run the helper directly.
It exercises phone pairing, desktop follower mode, authenticated reads, refusal
of host-only writes, shared-thread reset, offline/reconnect with updated state,
and revocation on both devices. It uses no external model calls. The runner
prints the retained screenshots and logs directory under the system temporary
folder (`draft-assistant-companion-browser-rehearsal-*/browser-evidence`).

For the draft-night UI, also check gear → **All settings**, saving the current
league account as your default identity, independent player-list scrolling,
and expanding/collapsing the AI model picker. A saved account before the draft
order appears should show a pending seat rather than inventing a pick position.

### End-to-end, for real

```bash
npm run test:e2e
```

One test. It launches the built app, waits for the launch screen to resolve,
and asserts a real screen is on it — the header with the league name and the
Draft/Season toggle if a league is configured, the setup form if not.
Deliberately narrow: it is the only test here that loads the built bundle
into a WKWebView under the production CSP and sends an `invoke()` over the
real IPC bridge, so it aims at what that seam produces — a window that opens
blank, or never gets past "Restoring…".

Real, not headless: the session reports as `webkit 605.1.15 macos`, and on a
machine with a league saved the spec logs what it read off the screen:

```
[e2e] resolved on .app-header; screen reads:
UMass Wrestling Fantasy Football LeagueWeek 1 · 0–0 · 7th of 14SeasonDraft
14-team full-PPR · 15 roundsLive · 0s agoAsk AIThis weekvs Meatball ·
127.6 – 125.4Win odds52%Playoffs44%Locks in10d 18h…
```

The script builds with `tauri build --features wdio --no-bundle` into
`src-tauri/target/wdio/`, then points WebdriverIO at the binary. About a
minute of build and a minute of run, warm.

It has to be the **Tauri CLI, not `cargo build --release`**:
`generate_context!` embeds `dist/` only when `tauri/custom-protocol` is on
and the CLI is what turns it on, so plain cargo aims the webview at the
`devUrl` instead. That fails quietly — blank unless a Vite dev server happens
to be up, green against the dev server rather than the built bundle if one
is. The script gets it right; the trap is for driving the binary by hand.

#### How it works on macOS, and why that is safe

`tauri-driver` — the usual answer — is Windows and Linux only; its own README
lists macOS as Todo, and there is no WKWebView driver binary to point it at.
`@wdio/tauri-service` gets around this with its default `embedded` provider:
the WebDriver server runs _inside_ the app, from the
`tauri-plugin-wdio-webdriver` crate. Nothing external to install.

That is a full remote-control surface, so it sits behind a cargo feature that
is off by default — in three places, all switched by the same flag:

- **The plugins.** `optional = true` in `Cargo.toml`, pulled in only by
  `[features] wdio`. Absent from the dependency graph, not merely unused.
- **The registration.** The two `.plugin(...)` calls in `lib.rs` are under
  `#[cfg(feature = "wdio")]`.
- **The permission.** `capabilities/wdio.json` grants `wdio:default`, and
  `build.rs` only feeds it to `tauri-build` when `CARGO_FEATURE_WDIO` is set.
  This one needs a build script rather than a `cfg`: a capability is baked
  into the ACL at compile time and cannot be revoked at runtime, so a grant
  left in `default.json` would ship a permission for a plugin that is not
  there — and re-arm silently the moment anyone added it back.

Tauri's ACL makes that last one self-enforcing rather than a thing to
remember. Feed `capabilities/wdio.json` to a build without the feature and it
does not warn, it refuses:

```
failed to run tauri-build: Permission wdio:default not found,
expected one of core:default, core:app:default, …
```

Check that a default build excludes the WebDriver plugins. Axum is an ordinary
dependency used by the optional companion server and is present in default
builds; its presence does not mean WebDriver is enabled.

Example checks (symbol counts vary with build and toolchain):

```
$ cargo tree -e normal | grep -i wdio                          # nothing
$ cargo tree -e normal --features wdio | grep -i wdio
├── tauri-plugin-wdio … └── tauri-plugin-wdio-webdriver
$ strings -a target/release/draft-assistant      | grep -ci wdio  # 0
$ strings -a target/wdio/release/draft-assistant | grep -ci wdio  # 48
$ nm -a target/debug/draft-assistant             | grep -ci wdio  # 0
$ nm -a target/wdio/debug/draft-assistant        | grep -ci wdio  # 3228
```

That build also goes to `src-tauri/target/wdio/`, so the binary with a
WebDriver server in it never sits where `npm run tauri build` writes.

The service also offers `browser.tauri.execute()` and command mocking, and
this uses neither: both need `withGlobalTauri: true` plus an
`@wdio/tauri-plugin` import in the app's own entry point — a global
`__TAURI__` on `window`, and test code inside the shipped `index-*.js`. Not
worth adding to production for a smoke test, so the run drives plain
WebDriver and the service logs `Tauri core.invoke not available` every few
seconds while probing for what is not there. Expected noise; still green.

#### Why it is not in `verify`, and where it is instead

Its own workflow, `.github/workflows/e2e.yml` — on pushes to `main`, on PRs
touching the seam it watches (`lib.rs`, `capabilities/`, `build.rs`,
`tauri.conf.json`, `vite.config.ts`, `e2e/`), and on `workflow_dispatch`.

It stays out of the PR gate because it is expensive and can fail for reasons
that are not the diff's fault: `--features wdio` is a different feature set
from everything `verify` builds and shares no artifacts with it — a second
full compile of `tauri`, `wry`, `reqwest`, and the WebDriver plugins, on
a runner billed at ten times the Linux rate — and it needs a real window and
the live Sleeper API. Its npm side is another ~152 MB.

Which is why `e2e/` is **its own npm package with its own lockfile** rather
than devDependencies of the app. Everyday `npm ci` stays at 209 MB instead of
324 MB — and, the sharper reason, WebdriverIO's tree carries high-severity
advisories with no fix available (`deepmerge-ts` and everything above it), so
keeping it out means the app tree's `npm audit --audit-level=high` still
reports `found 0 vulnerabilities`. The e2e tree is audited too, through
`scripts/check-npm-audit.mjs`, with that one advisory allow-listed by id
rather than the level being turned down to accommodate a test harness.

For the same reason `npm run lint:rust` is on default features rather than
`--all-features`: asking `verify` for both feature sets would double its Rust
compile. The `#[cfg(feature = "wdio")]` path is linted at the same
`-D warnings` by `npm run lint:rust:wdio`, in the e2e job that already pays
for that build.
