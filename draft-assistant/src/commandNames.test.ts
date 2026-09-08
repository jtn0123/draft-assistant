// The other half of the command contract.
//
// `src-tauri/tests/command_surface.rs` proves every name in
// `generate_handler!` reaches a real command. Nothing proved the reverse: that
// the names this app actually types into `invoke()` are in that list. Rename a
// command in Rust, update `lib.rs` and the Rust test, and the frontend still
// asks for the old name — it compiles, it ships, and it fails at the user with
// "Command not found" the first time they click the thing.
//
// The same goes for events. The backend emits by string and the frontend
// listens by string, with a type parameter on each side that agrees with
// nothing. `chat-progress` is the newest of them, and the streaming panel goes
// quiet if either end is renamed alone.

import { readFileSync } from "node:fs";
import { readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { expect, it } from "vitest";

const read = (path: string) => readFileSync(path, "utf8");

/** Every `.rs` file under the backend that is not itself a test.
 *
 *  The test files are excluded on purpose: they name the events too, and a
 *  scan that counted them found `chat-progress` in the test that listens for
 *  it after the emit had been renamed — a guard that cannot fail. */
function rustSources(dir = "src-tauri/src"): string[] {
  return readdirSync(dir).flatMap((entry) => {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) return rustSources(path);
    if (!path.endsWith(".rs") || /_?tests?\.rs$/.test(entry)) return [];
    return [path];
  });
}

/** The names inside `tauri::generate_handler![ ... ]`, module paths stripped. */
function registeredCommands(): Set<string> {
  const source = read("src-tauri/src/lib.rs");
  const marker = "tauri::generate_handler![";
  const start = source.indexOf(marker);
  expect(start, "lib.rs has a generate_handler! list").toBeGreaterThan(-1);
  const body = source.slice(start + marker.length).split("]")[0];
  return new Set(
    body
      .split(",")
      .map((name) => name.trim().split("::").pop() ?? "")
      .filter((name) => /^[a-z_][a-z0-9_]*$/.test(name)),
  );
}

/** Every string this file hands to `invoke`, wherever the call is written.
 *
 *  `invokeView` and `invokeSeason` are `invoke` with a validator wrapped round
 *  it; reading only the bare `invoke(` calls left a third of the surface
 *  unguarded, which is exactly the sort of hole this file exists to close. */
function invoked(path: string): string[] {
  const pattern = /(?:invoke|invokeView|invokeSeason)(?:<[^>]*>)?\(\s*"([a-z0-9_]+)"/g;
  return [...read(path).matchAll(pattern)].map((hit) => hit[1]);
}

/** Every event name this file subscribes to. */
function listened(path: string): string[] {
  return [...read(path).matchAll(/listen(?:<[^>]*>)?\(\s*"([a-z0-9-]+)"/g)].map((hit) => hit[1]);
}

it("every command the desktop app invokes is registered in lib.rs", () => {
  const registered = registeredCommands();
  const asked = invoked("src/api.ts");

  expect(asked.length, "api.ts still invokes commands").toBeGreaterThan(20);
  const missing = [...new Set(asked)].filter((name) => !registered.has(name)).sort();
  expect(missing, `commands the frontend asks for that lib.rs does not register`).toEqual([]);
});

// The follower reaches its host over HTTP for most things, but the handful it
// still runs locally go through the same dispatcher.
it("every command the follower backend invokes is registered too", () => {
  const registered = registeredCommands();
  const asked = [...new Set(invoked("src/apiRemote.ts"))];

  const missing = asked.filter((name) => !registered.has(name)).sort();
  expect(missing).toEqual([]);
});

it("every event the app listens for is emitted by the backend", () => {
  const sources = rustSources().map(read).join("\n");
  const events = [...new Set([...listened("src/api.ts"), ...listened("src/apiRemote.ts")])].sort();

  expect(events, "the app still listens for events").not.toHaveLength(0);
  const orphaned = events.filter((name) => !sources.includes(`"${name}"`));
  expect(orphaned, "events listened for that nothing in src-tauri emits").toEqual([]);
});

// Not a failure when it drifts — a command may be reached from the companion
// server or a rehearsal script rather than from `api.ts` — but the count is
// pinned so a command going unused is noticed rather than accumulating.
it("names the registered commands no frontend path asks for", () => {
  const asked = new Set([...invoked("src/api.ts"), ...invoked("src/apiRemote.ts")]);
  const unused = [...registeredCommands()].filter((name) => !asked.has(name)).sort();

  expect(unused).toEqual([
    // Reached past `api` on purpose, straight to Tauri: see chatCancel.ts.
    "cancel_claude",
    // Yahoo auction leagues: the backend can read a budget and the costs
    // paid, and no screen asks for either yet.
    "yahoo_auction",
  ]);
});
