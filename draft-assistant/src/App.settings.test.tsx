// Grade item D6. The settings menu is where most of the shell's behaviour
// lives — six rows, each one an action against the backend that can fail —
// and almost none of it was reached by a test. Everything here is asserted
// through what a user sees on the row or in the message underneath.

import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./api", async () => ({ api: (await import("./test/appHarness")).harness().api }));

import App from "./App";
import { setAvatarMode } from "./avatars";
import { resetPrefs } from "./prefs";
import { resetThemePreference } from "./theme";
import { settle } from "./test/settle";
import { draftFixture, fakeStorage, harness, restoringConfig } from "./test/appHarness";
import type { DraftView } from "./types";

const h = harness();

/** Load the app on the draft board with a league already saved. */
async function loaded(view = draftFixture()) {
  h.api.getConfig.mockResolvedValue(restoringConfig(view));
  h.api.addLeague.mockResolvedValue(view);
  render(<App />);
  await screen.findByText(view.league.name);
}

/** Open the settings menu and click one of its rows. */
async function chooseSetting(label: RegExp) {
  await settle(() => {
    const gear = screen.queryByRole("button", { name: "Settings" });
    if (gear !== null && screen.queryByRole("menu") === null) gear.click();
  });
  await settle(() => {
    screen.getByRole("menuitemcheckbox", { name: label }).click();
  });
}

function settingsRow(label: RegExp): HTMLElement {
  return screen.getByRole("menuitemcheckbox", { name: label });
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

describe("the pick chime row", () => {
  it("turns the chime off and says so on the row", async () => {
    await loaded();
    await chooseSetting(/Pick chime/);
    expect(settingsRow(/Pick chime/)).toHaveAttribute("aria-checked", "false");
    expect(settingsRow(/Pick chime/)).toHaveTextContent("Off");

    await settle(() => {
      settingsRow(/Pick chime/).click();
    });
    expect(settingsRow(/Pick chime/)).toHaveAttribute("aria-checked", "true");
    expect(settingsRow(/Pick chime/)).toHaveTextContent("On");
  });
});

describe("the live sync row", () => {
  it("stops and restarts polling, and announces the restart", async () => {
    await loaded();
    await waitFor(() => expect(h.api.startPolling).toHaveBeenCalledTimes(1));

    await chooseSetting(/Live sync/);
    expect(h.api.stopPolling).toHaveBeenCalledTimes(1);
    expect(settingsRow(/Live sync/)).toHaveTextContent("Not polling Sleeper");
    expect(settingsRow(/Live sync/)).toHaveTextContent("Off");

    await settle(() => {
      settingsRow(/Live sync/).click();
    });
    expect(h.api.startPolling).toHaveBeenCalledTimes(2);
    expect(screen.getByText("Live sync on — polling Sleeper every 3s")).toBeInTheDocument();
    // Once polling, the row reports how long ago the last sync landed.
    expect(settingsRow(/Live sync/)).toHaveTextContent(/Last sync/);
  });

  it("says so when live sync cannot be turned off, and offers another go", async () => {
    h.api.stopPolling.mockRejectedValue(new Error("backend is not listening"));
    await loaded();
    await chooseSetting(/Live sync/);

    const failure = screen.getByRole("alert");
    expect(failure).toHaveTextContent("Could not change live sync — backend is not listening");
    await settle(() => {
      screen.getByRole("button", { name: "Try again" }).click();
    });
    expect(h.api.stopPolling).toHaveBeenCalledTimes(2);
  });

  it("says so when live sync could not be started at launch", async () => {
    h.api.startPolling.mockRejectedValueOnce(new Error("no draft to poll"));
    await loaded();

    const failure = await screen.findByRole("alert");
    expect(failure).toHaveTextContent("Could not turn live sync on — no draft to poll");
    await settle(() => {
      screen.getByRole("button", { name: "Try again" }).click();
    });
    expect(h.api.startPolling).toHaveBeenCalledTimes(2);
  });
});

describe("the export row", () => {
  it("names the file it wrote, and closes the menu on the way", async () => {
    await loaded();
    await chooseSetting(/Export state/);
    expect(await screen.findByText("State exported: /tmp/draft-state.json")).toBeInTheDocument();
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("keeps its own words when the backend gives no reason at all", async () => {
    // A rejection with nothing in it must not produce a trailing em dash.
    h.api.exportState.mockRejectedValue("");
    await loaded();
    await chooseSetting(/Export state/);

    const failure = await screen.findByRole("alert");
    expect(failure).toHaveTextContent("Could not export the state");
    expect(failure.textContent).not.toContain("—");

    await settle(() => {
      screen.getByRole("button", { name: "Try again" }).click();
    });
    expect(h.api.exportState).toHaveBeenCalledTimes(2);
  });
});

describe("the refresh row", () => {
  it("says why a refresh failed and can be run again", async () => {
    h.api.refreshData.mockRejectedValue(new Error("projections are down"));
    await loaded();
    await chooseSetting(/Refresh data/);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not refresh the projections — projections are down",
    );
    await settle(() => {
      screen.getByRole("button", { name: "Try again" }).click();
    });
    expect(h.api.refreshData).toHaveBeenCalledTimes(2);
  });
});

describe("the undo action", () => {
  it("says why an undo failed and can be run again", async () => {
    h.api.undoManualPick.mockRejectedValue(new Error("nothing to undo"));
    await loaded();

    await settle(() => {
      screen.getByRole("button", { name: "Undo" }).click();
    });
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not undo the last recorded pick — nothing to undo",
    );
    await settle(() => {
      screen.getByRole("button", { name: "Try again" }).click();
    });
    expect(h.api.undoManualPick).toHaveBeenCalledTimes(2);
  });
});

describe("the player pictures row", () => {
  it("switches to team logos and explains what that means", async () => {
    await loaded();
    await chooseSetting(/Player pictures/);

    const row = settingsRow(/Player pictures/);
    expect(row).toHaveTextContent("Team logos only — no photo downloads");
    expect(row).toHaveTextContent("Team logos");
    expect(row).toHaveAttribute("aria-checked", "false");

    await settle(() => {
      settingsRow(/Player pictures/).click();
    });
    expect(settingsRow(/Player pictures/)).toHaveTextContent(/Headshots from Sleeper/);
    expect(settingsRow(/Player pictures/)).toHaveAttribute("aria-checked", "true");
  });
});

describe("the appearance row", () => {
  it("steps from following the system to an explicit light and dark", async () => {
    await loaded();
    await settle(() => {
      screen.getByRole("button", { name: "Settings" }).click();
    });

    const row = () => settingsRow(/Appearance/);
    expect(row()).toHaveTextContent("Following your system setting");
    expect(row()).toHaveTextContent("System (light)");

    await settle(() => {
      row().click();
    });
    expect(row()).toHaveTextContent("Overriding your system setting");
    expect(row()).toHaveTextContent("Light");
    expect(document.documentElement.dataset.theme).toBe("light");

    await settle(() => {
      row().click();
    });
    expect(row()).toHaveTextContent("Dark");
    expect(row()).toHaveAttribute("aria-checked", "true");
    expect(document.documentElement.dataset.theme).toBe("dark");

    await settle(() => {
      row().click();
    });
    expect(row()).toHaveTextContent("System (light)");
  });
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

describe("the league row", () => {
  /** The saved config, with a second league already loaded once. */
  function twoLeagues(view: DraftView) {
    return {
      ...restoringConfig(view),
      leagues: [
        { league_id: view.league.league_id, name: view.league.name, season: "2026" },
        { league_id: "2222222222222222222", name: "Mock draft", season: "2026" },
      ],
    };
  }

  it("opens the picker with every league the app has loaded", async () => {
    const view = draftFixture();
    h.api.getConfig.mockResolvedValue(twoLeagues(view));
    h.api.addLeague.mockResolvedValue(view);
    render(<App />);
    await screen.findByText(view.league.name);

    await chooseSetting(/League/);
    expect(screen.getByRole("dialog", { name: "Switch league" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Mock draft/ })).toBeInTheDocument();
  });

  it("stops the old poller, loads the new league, and restarts live sync", async () => {
    const view = draftFixture();
    const other = draftFixture();
    other.league = { ...other.league, league_id: "2222222222222222222", name: "Mock draft" };
    h.api.getConfig.mockResolvedValue(twoLeagues(view));
    h.api.addLeague.mockResolvedValue(view);
    render(<App />);
    await screen.findByText(view.league.name);
    await waitFor(() => expect(h.api.startPolling).toHaveBeenCalledTimes(1));

    h.api.addLeague.mockResolvedValue(other);
    await chooseSetting(/League/);
    await settle(() => {
      screen.getByRole("button", { name: /Mock draft/ }).click();
    });

    await screen.findByText(/Switched to Mock draft/);
    expect(h.api.addLeague).toHaveBeenLastCalledWith("2222222222222222222");
    // The 3-second poller was told to stop before the switch, so it cannot
    // write the old draft's picks over the new board on its way out.
    expect(h.api.stopPolling).toHaveBeenCalledTimes(1);
    expect(h.api.startPolling).toHaveBeenCalledTimes(2);
    expect(screen.getByText("Mock draft")).toBeInTheDocument();
  });

  it("says so and offers a retry when the new league will not load", async () => {
    const view = draftFixture();
    h.api.getConfig.mockResolvedValue(twoLeagues(view));
    h.api.addLeague.mockResolvedValue(view);
    render(<App />);
    await screen.findByText(view.league.name);

    h.api.addLeague.mockRejectedValue(new Error("league 2222222222222222222 not found"));
    await chooseSetting(/League/);
    await settle(() => {
      screen.getByRole("button", { name: /Mock draft/ }).click();
    });

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Could not switch leagues");
    expect(alert).toHaveTextContent("not found");
    // The league that was on screen is still the one on screen.
    expect(screen.getByText(view.league.name)).toBeInTheDocument();
  });
});
