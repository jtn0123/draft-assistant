// Grade item D6. The shell's screen switch: which of the three states the
// season side can be in, the chat panel beside it, the launch card before any
// of it, and the confirm dialog over the top. These are the paths a user hits
// on a bad network, and none of them were reached.

import { act, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./api", async () => ({ api: (await import("./test/appHarness")).harness().api }));

import App from "./App";
import { resetPrefs } from "./prefs";
import { resetThemePreference } from "./theme";
import { settle } from "./test/settle";
import {
  draftFixture,
  fakeStorage,
  harness,
  restoringConfig,
  seasonFixture,
} from "./test/appHarness";

const h = harness();

async function loaded(view = draftFixture()) {
  h.api.getConfig.mockResolvedValue(restoringConfig(view));
  h.api.addLeague.mockResolvedValue(view);
  render(<App />);
  await screen.findByText(view.league.name);
}

beforeEach(() => {
  h.reset();
  // jsdom has no Element.scrollTo, and the chat thread scrolls itself.
  Element.prototype.scrollTo = vi.fn();
  fakeStorage({ "da.screen": "draft" });
  resetPrefs();
  resetThemePreference();
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("the season screen before it has anything to show", () => {
  it("says it is loading rather than showing an empty week", async () => {
    fakeStorage({ "da.screen": "season" });
    resetPrefs();
    // A load that never answers: the state between opening the tab and the
    // first view arriving.
    h.api.loadSeason.mockReturnValue(new Promise(() => undefined));
    await loaded();

    expect(screen.getByText("Loading this week…")).toBeInTheDocument();
    // The header still says which season, since the week is not known yet.
    expect(screen.getByText("2026 season")).toBeInTheDocument();
  });

  it("names the week and the user's record once it has", async () => {
    fakeStorage({ "da.screen": "season" });
    resetPrefs();
    h.api.loadSeason.mockResolvedValue(seasonFixture());
    await loaded();

    expect(await screen.findByText("Week 3 · 2–0 · 2nd of 1")).toBeInTheDocument();
  });

  it("counts the league instead when none of the standings are the user's", async () => {
    fakeStorage({ "da.screen": "season" });
    resetPrefs();
    const view = seasonFixture();
    view.standings = view.standings.map((row) => ({ ...row, is_mine: false }));
    h.api.loadSeason.mockResolvedValue(view);
    await loaded();

    expect(await screen.findByText("Week 3 · 1 teams")).toBeInTheDocument();
  });

  it("rebuilds the season too when the projections are refreshed under it", async () => {
    fakeStorage({ "da.screen": "season" });
    resetPrefs();
    const refreshed = draftFixture();
    refreshed.data_health.board_size = 312;
    h.api.loadSeason.mockResolvedValue(seasonFixture());
    h.api.refreshData.mockResolvedValue(refreshed);
    await loaded();
    await screen.findByText("Week 3 · 2–0 · 2nd of 1");
    expect(h.api.loadSeason).toHaveBeenCalledTimes(1);

    await settle(() => {
      screen.getByRole("button", { name: "Settings" }).click();
    });
    await settle(() => {
      screen.getByRole("menuitemcheckbox", { name: /Refresh data/ }).click();
    });

    // The board was rebuilt from new projections, so the season screen is
    // sitting on stale numbers until it reloads — and it does, forcing past
    // the cache that produced them.
    await waitFor(() => expect(h.api.loadSeason).toHaveBeenCalledTimes(2));
    expect(h.api.loadSeason).toHaveBeenLastCalledWith(true);
  });
});

describe("the chat panel", () => {
  it("opens beside the draft board and tells Claude which pick it is", async () => {
    await loaded();
    await settle(() => {
      screen.getByRole("button", { name: "Ask Claude" }).click();
    });

    // Pick 27 of a 14-team draft is 2.13.
    expect(await screen.findByText(/Sees this draft · pick 2.13/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Ask Claude" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );

    await settle(() => {
      screen.getByRole("button", { name: "Close" }).click();
    });
    await waitFor(() => expect(screen.queryByText(/Sees this draft/)).not.toBeInTheDocument());
  });

  it("tells Claude about the week instead when the season screen is showing", async () => {
    fakeStorage({ "da.screen": "season" });
    resetPrefs();
    h.api.loadSeason.mockResolvedValue(seasonFixture());
    await loaded();
    await screen.findByText("Week 3 · 2–0 · 2nd of 1");

    await settle(() => {
      screen.getByRole("button", { name: "Ask Claude" }).click();
    });
    expect(await screen.findByText(/Sees week 3 · your lineup and the league/)).toBeInTheDocument();
  });
});

describe("the confirm dialog", () => {
  it("records nothing when it is dismissed", async () => {
    await loaded();
    const rows = await waitFor(() => {
      const buttons = screen.getAllByRole("button", { name: "Draft" });
      expect(buttons.length).toBeGreaterThan(1);
      return buttons;
    });

    await settle(() => {
      rows[rows.length - 1].click();
    });
    const dialog = screen.getByRole("dialog");
    // The pick it is about is named on the dialog, not just the player.
    expect(dialog).toHaveTextContent("Pick 2.13 · slot 2");

    await settle(() => {
      within(dialog).getByRole("button", { name: "Cancel" }).click();
    });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(h.api.recordManualPick).not.toHaveBeenCalled();
  });
});

describe("the launch card", () => {
  it("reconnects on request rather than making the user start again", async () => {
    const view = draftFixture();
    h.api.getConfig.mockResolvedValue(restoringConfig(view));
    h.api.addLeague
      .mockRejectedValueOnce(new Error("request timed out"))
      .mockResolvedValueOnce(view);
    render(<App />);

    await settle(() => {
      // Nothing to do yet; wait for the failure to land.
    });
    const retry = await screen.findByRole("button", { name: "Try again" });
    await settle(() => {
      retry.click();
    });

    expect(await screen.findByText(view.league.name)).toBeInTheDocument();
    expect(h.api.addLeague).toHaveBeenCalledTimes(2);
  });

  it("offers a way out to a different league when reconnecting keeps failing", async () => {
    const view = draftFixture();
    h.api.getConfig.mockResolvedValue(restoringConfig(view));
    h.api.addLeague.mockRejectedValue(new Error("league is gone"));
    render(<App />);

    await settle(() => {
      screen.queryByRole("button", { name: "Enter a different league" })?.click();
    });
    await settle(() => {
      screen.getByRole("button", { name: "Enter a different league" }).click();
    });
    expect(await screen.findByLabelText("League ID")).toBeInTheDocument();
  });

  it("goes straight to the board and starts polling once setup succeeds", async () => {
    const view = draftFixture();
    h.api.getConfig.mockResolvedValue({
      my_user_id: null,
      active_league_id: null,
      leagues: [],
    });
    h.api.addLeague.mockResolvedValue(view);
    render(<App />);

    const field = await screen.findByLabelText("League ID");
    await settle(() => {
      field.focus();
    });
    act(() => {
      Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")?.set?.call(
        field,
        view.league.league_id,
      );
      field.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await settle(() => {
      screen.getByRole("button", { name: "Load league" }).click();
    });

    expect(await screen.findByText(view.league.name)).toBeInTheDocument();
    await waitFor(() => expect(h.api.startPolling).toHaveBeenCalledWith(3));
  });
});

describe("the header meta line", () => {
  it("mentions manual picks only while they are in play", async () => {
    const plain = draftFixture();
    plain.draft.manual_picks_active = false;
    await loaded(plain);
    expect(screen.getByText(/^14-team full-PPR · 15 rounds$/)).toBeInTheDocument();
  });
});

describe("a live update with no poll history behind it", () => {
  it("dates the sync from the view itself rather than claiming nothing", async () => {
    await loaded();
    const pushed = draftFixture();
    pushed.league.name = "League updated by poll";
    pushed.data_health.poll_last_success_at = null;
    pushed.data_health.poll_last_error = null;

    act(() => h.push.draft?.(pushed));
    expect(screen.getByText("League updated by poll")).toBeInTheDocument();
    // No failures reported, so the pill still reads as live rather than stale.
    expect(screen.getByText(/^Live · /)).toBeInTheDocument();
  });
});

describe("closing the window", () => {
  it("lets go of both backend subscriptions", async () => {
    const stopDraft = vi.fn();
    const stopHealth = vi.fn();
    h.api.onDraftUpdated.mockReturnValue(Promise.resolve(stopDraft));
    h.api.onPollHealth.mockReturnValue(Promise.resolve(stopHealth));

    const view = draftFixture();
    h.api.getConfig.mockResolvedValue(restoringConfig(view));
    h.api.addLeague.mockResolvedValue(view);
    const { unmount } = render(<App />);
    await screen.findByText(view.league.name);

    await act(async () => {
      unmount();
      await Promise.resolve();
    });
    expect(stopDraft).toHaveBeenCalledTimes(1);
    expect(stopHealth).toHaveBeenCalledTimes(1);
  });
});
