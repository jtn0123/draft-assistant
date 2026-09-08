// The Cancel button's one call.
//
// It is the only place in the app that reaches past `api` straight to Tauri,
// which is exactly why it needs its own test: nothing else exercises the
// guard that keeps it quiet in the browser preview, or the promise it is not
// allowed to reject from. A cancel that threw would land in the panel as an
// error beside an answer that is still, correctly, on its way.

import { afterEach, beforeEach, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { cancelClaude } from "./chatCancel";

/** The marker Tauri puts on the window, and the browser preview never has. */
function onTheDesktop(present: boolean) {
  if (present) {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  } else {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  }
}

beforeEach(() => {
  invoke.mockReset();
});

afterEach(() => {
  onTheDesktop(false);
});

it("asks the desktop backend to stop this screen's answer", async () => {
  onTheDesktop(true);
  invoke.mockResolvedValue(true);

  await expect(cancelClaude("draft")).resolves.toBe(true);
  expect(invoke).toHaveBeenCalledWith("cancel_claude", { screen: "draft" });
});

it("says there was nothing to stop when the backend says so", async () => {
  onTheDesktop(true);
  invoke.mockResolvedValue(false);

  await expect(cancelClaude("season")).resolves.toBe(false);
});

// The follower's remote backend and the browser preview have no answer of
// their own in flight, and `invoke` there would throw into a panel that is
// still waiting.
it("does nothing at all outside the desktop app", async () => {
  onTheDesktop(false);

  await expect(cancelClaude("draft")).resolves.toBe(false);
  expect(invoke).not.toHaveBeenCalled();
});

// A cancel that failed leaves the answer running, which the panel is already
// showing. Rejecting would put an error on screen for a turn that is fine.
it("never rejects when the call itself fails", async () => {
  onTheDesktop(true);
  invoke.mockRejectedValue(new Error("the window went away"));

  await expect(cancelClaude("draft")).resolves.toBe(false);
});
