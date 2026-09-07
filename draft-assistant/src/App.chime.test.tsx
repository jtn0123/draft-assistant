// The on-the-clock chime, tested through the whole app: it fires when the
// clock reaches you, and it never takes the draft down with it when the audio
// stack misbehaves. Split from App.settings.test.tsx, which was over the
// 500-line cap and where the chime was the one thing not about a settings row.

import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./api", async () => ({ api: (await import("./test/appHarness")).harness().api }));

import "./test/warmScreens";
import App from "./App";
import { setAvatarMode } from "./avatars";
import { resetPrefs } from "./prefs";
import { resetThemePreference } from "./theme";
import { settle } from "./test/settle";
import { draftFixture, fakeStorage, harness, restoringConfig } from "./test/appHarness";

const h = harness();

/** Load the app on the draft board with a league already saved; see the
 *  note on the same helper in App.settings.test.tsx for why it settles. */
async function loaded(view = draftFixture()) {
  h.api.getConfig.mockResolvedValue(restoringConfig(view));
  h.api.addLeague.mockResolvedValue(view);
  render(<App />);
  await screen.findByRole("heading", { name: view.league.name });
  await settle(() => {});
}

beforeEach(() => {
  h.reset();
  fakeStorage({ "da.screen": "draft" });
  resetPrefs();
  resetThemePreference();
  setAvatarMode("headshots");
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

// The chime is the one thing this app is allowed to interrupt a user with, so
// it has to be right in both directions: it fires when the clock reaches you,
// and it never takes the draft down with it when the audio stack misbehaves.
describe("the on-the-clock chime", () => {
  interface AudioSpy {
    created: number;
    closed: number;
    tones: number;
    /** One entry per context, in creation order, each flipped by that
     * context's own `close()`. `playChime` schedules its close on a 600ms
     * timer, so a chime played before a test switches to fake timers keeps a
     * real timeout that lands whenever the machine gets round to it — under
     * parallel load, possibly mid-assertion. Anything asserting *which*
     * context was let go reads these rather than the running count. */
    contexts: { closed: boolean }[];
  }

  /** A WebAudio stack this test can count, installed as a real constructor —
   * `playChime` calls `new` on it, and a plain arrow would hand back an empty
   * object and be swallowed by the very catch this is meant to avoid. */
  function stubAudio(): AudioSpy {
    const spy: AudioSpy = { created: 0, closed: 0, tones: 0, contexts: [] };
    class FakeAudioContext {
      currentTime = 0;
      destination = {};
      /** This context's own entry in `spy.contexts`. */
      mine: { closed: boolean };
      constructor() {
        spy.created += 1;
        this.mine = { closed: false };
        spy.contexts.push(this.mine);
      }
      createOscillator() {
        spy.tones += 1;
        return {
          frequency: { value: 0 },
          type: "",
          connect: () => ({ connect: () => undefined }),
          start: () => undefined,
          stop: () => undefined,
        };
      }
      createGain() {
        return {
          gain: {
            setValueAtTime: () => undefined,
            exponentialRampToValueAtTime: () => undefined,
          },
          connect: () => undefined,
        };
      }
      close() {
        spy.closed += 1;
        this.mine.closed = true;
        return Promise.resolve();
      }
    }
    vi.stubGlobal("AudioContext", FakeAudioContext);
    return spy;
  }

  it("plays when the clock reaches you, and lets go of the audio context after", async () => {
    const audio = stubAudio();
    const mine = draftFixture();
    mine.draft.is_my_pick = true;
    await loaded(mine);

    // Two tones, one context, played once.
    expect(audio.created).toBe(1);
    expect(audio.tones).toBe(2);

    // The clock moves on and comes back round: a second turn is a second
    // chime, not a one-per-session event.
    vi.useFakeTimers();
    const notMine = draftFixture();
    notMine.draft.is_my_pick = false;
    act(() => h.push.draft?.(notMine));
    expect(audio.created).toBe(1);
    act(() => h.push.draft?.(mine));
    expect(audio.created).toBe(2);

    // The context is short-lived: an app that leaves one open per pick would
    // run a draft out of them. Asserted against the second chime's own
    // context, whose 600ms close this test scheduled and owns — the first
    // chime's was scheduled on real timers back in `loaded`, and a slow
    // enough machine lands it anywhere in here.
    const second = audio.contexts[1];
    expect(second.closed).toBe(false);
    act(() => {
      vi.advanceTimersByTime(600);
    });
    expect(second.closed).toBe(true);
  });

  it("stays silent when the chime has been muted", async () => {
    const audio = stubAudio();
    fakeStorage({ "da.screen": "draft", "da.chime": "off" });
    resetPrefs();
    const mine = draftFixture();
    mine.draft.is_my_pick = true;
    await loaded(mine);

    expect(audio.created).toBe(0);
  });

  it("never lets a broken audio stack interrupt the draft", async () => {
    class BrokenAudioContext {
      constructor() {
        throw new Error("audio device is unavailable");
      }
    }
    vi.stubGlobal("AudioContext", BrokenAudioContext);
    const mine = draftFixture();
    mine.draft.is_my_pick = true;
    await loaded(mine);

    // The board is up and there is no error on screen: the failure was
    // swallowed exactly where it happened.
    expect(screen.getByText(mine.league.name)).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("does nothing at all in a webview with no WebAudio", async () => {
    vi.stubGlobal("AudioContext", undefined);
    vi.stubGlobal("webkitAudioContext", undefined);
    const mine = draftFixture();
    mine.draft.is_my_pick = true;
    await loaded(mine);
    expect(screen.getByText(mine.league.name)).toBeInTheDocument();
  });
});
