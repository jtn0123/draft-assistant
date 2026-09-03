import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DraftView } from "./types";

vi.mock("./api", async () => ({ api: (await import("./test/appHarness")).harness().api }));

import "./test/warmScreens";
import App from "./App";
import { resetPrefs } from "./prefs";
import { settle } from "./test/settle";
import {
  draftFixture,
  fakeStorage,
  harness,
  restoringConfig,
  seasonFixture,
} from "./test/appHarness";

const h = harness();

/**
 * The dev fixture with a deliberately short board.
 *
 * These tests drive workflow — setup, errors, toasts, tabs — not board volume,
 * and rendering the full 393-player board in every test is what pushed this
 * file past the 5s budget under parallel worker load. Board rendering at scale
 * is covered by App.screens.test.tsx.
 */
function fixture(): DraftView {
  return draftFixture(24);
}

/**
 * The board's row actions, once the lazy DraftScreen chunk has arrived.
 *
 * The first "Draft" button is the Draft/Season mode toggle, so a row action
 * only exists when more than one matches. Resolving that chunk is a real
 * dynamic import, and under parallel worker load it does not reliably finish
 * inside waitFor's 1s default — which is the whole of this flake. The
 * assertion is unchanged; it just gets the time an import can take.
 */
async function rowDraftButtons(): Promise<HTMLElement[]> {
  return waitFor(
    () => {
      const buttons = screen.getAllByRole("button", { name: "Draft" });
      expect(buttons.length).toBeGreaterThan(1);
      return buttons;
    },
    { timeout: 5000 },
  );
}

beforeEach(() => {
  // The app opens on Season by default; these tests drive the draft board.
  // jsdom here has no storage, so give it a scratch one.
  fakeStorage({ "da.screen": "draft" });
  // The preference stores hold this session's choices; each test starts from
  // what its own storage says.
  resetPrefs();
  h.reset();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("App live workflow", () => {
  it("shows setup only after confirming there is no saved league", async () => {
    h.api.getConfig.mockResolvedValue({
      my_user_id: null,
      active_league_id: null,
      leagues: [],
    });

    render(<App />);
    // The launch card holds the screen until the config answers.
    expect(screen.getByText("Connecting to Sleeper")).toBeInTheDocument();
    expect(await screen.findByLabelText("League ID")).toBeInTheDocument();
  });

  it("loads live state, exposes failures, and supports manual pick and undo", async () => {
    const user = userEvent.setup();
    const initial = fixture();
    const afterPick = fixture();
    afterPick.available = afterPick.available.slice(1);
    afterPick.draft.total_picks_made += 1;
    h.api.getConfig.mockResolvedValue(restoringConfig(initial));
    h.api.addLeague.mockResolvedValue(initial);
    h.api.recordManualPick.mockResolvedValue(afterPick);
    h.api.undoManualPick.mockResolvedValue(initial);

    render(<App />);
    expect(await screen.findByRole("heading", { name: initial.league.name })).toBeInTheDocument();
    expect(screen.getByText(/^Live · /)).toBeInTheDocument();
    // Scoring format comes from the league's reception value (1.0 here).
    expect(screen.getByText(/^14-team full-PPR · \d+ rounds/)).toBeInTheDocument();

    const liveUpdate = fixture();
    liveUpdate.league.name = "League updated by poll";
    act(() => h.push.draft?.(liveUpdate));
    expect(screen.getByText("League updated by poll")).toBeInTheDocument();

    act(() => {
      h.push.health?.({
        last_success_at: initial.generated_at,
        consecutive_failures: 2,
        last_error: "network timeout",
      });
    });
    const stale = screen.getByText("Sync stale · 2 failures");
    expect(stale.closest("span")).toHaveAttribute("title", "network timeout");

    const rowDraft = await rowDraftButtons();
    await user.click(rowDraft[rowDraft.length - 1]);
    // "Mark drafted" is also the rec-card action, so confirm inside the dialog.
    const dialog = screen.getByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "Mark drafted" }));
    expect(h.api.recordManualPick).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole("button", { name: "Undo" }));
    expect(h.api.undoManualPick).toHaveBeenCalledTimes(1);
  });

  it("shows a setup error when a league cannot be loaded", async () => {
    const user = userEvent.setup();
    h.api.getConfig.mockResolvedValue({
      my_user_id: null,
      active_league_id: null,
      leagues: [],
    });
    h.api.addLeague.mockRejectedValue(new Error("league unavailable"));

    render(<App />);
    await user.type(await screen.findByLabelText("League ID"), "123456789012345");
    await user.click(screen.getByRole("button", { name: "Load league" }));
    expect(await screen.findByText("Error: league unavailable")).toBeInTheDocument();
  });

  it("offers to reconnect rather than dropping to setup when restore fails", async () => {
    const initial = fixture();
    h.api.getConfig.mockResolvedValue({
      my_user_id: null,
      active_league_id: initial.league.league_id,
      leagues: [
        {
          league_id: initial.league.league_id,
          name: "Dynasty Warriors",
          season: "2026",
          status: null,
        },
      ],
    });
    h.api.addLeague.mockRejectedValue(new Error("request timed out"));

    render(<App />);
    // A saved league that fails to load is a connection problem, not a reason
    // to make the user re-enter their league ID.
    expect(await screen.findByRole("button", { name: "Try again" })).toBeInTheDocument();
    expect(screen.getByText(/request timed out/)).toBeInTheDocument();
    expect(screen.queryByLabelText("League ID")).not.toBeInTheDocument();
    // The launch card names the league it is bringing back.
    expect(screen.getByText("Dynasty Warriors")).toBeInTheDocument();
    expect(screen.getByText(new RegExp(`\\(${initial.league.league_id}\\)`))).toBeInTheDocument();
  });

  it("reports how many players the rebuilt board covers after a refresh", async () => {
    const user = userEvent.setup();
    const initial = fixture();
    const refreshed = fixture();
    refreshed.data_health.board_size = 312;
    h.api.getConfig.mockResolvedValue(restoringConfig(initial));
    h.api.addLeague.mockResolvedValue(initial);
    h.api.refreshData.mockResolvedValue(refreshed);

    render(<App />);
    await screen.findByRole("heading", { name: initial.league.name });
    await user.click(screen.getByRole("button", { name: "Settings" }));
    // The settings rows are menu items with their own on/off state, not plain
    // buttons — see Header.test.tsx.
    await user.click(screen.getByRole("menuitemcheckbox", { name: /Refresh data/ }));
    expect(
      await screen.findByText("Projections refreshed — board rebuilt from 312 players"),
    ).toBeInTheDocument();
  });

  it("opens on the Season screen unless the draft board was the last choice", async () => {
    localStorage.removeItem("da.screen");
    const initial = fixture();
    h.api.getConfig.mockResolvedValue(restoringConfig(initial));
    h.api.addLeague.mockResolvedValue(initial);
    h.api.loadSeason.mockResolvedValue(seasonFixture());

    render(<App />);
    expect(await screen.findByText("vs punt_god · 122.4 – 108.9")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Season" })).toHaveAttribute("aria-pressed", "true");
  });

  it("loads the season view when the Season tab is opened", async () => {
    const user = userEvent.setup();
    const initial = fixture();
    h.api.getConfig.mockResolvedValue(restoringConfig(initial));
    h.api.addLeague.mockResolvedValue(initial);
    h.api.loadSeason.mockResolvedValue(seasonFixture());

    render(<App />);
    await screen.findByRole("heading", { name: initial.league.name });
    await user.click(screen.getByRole("button", { name: "Season" }));

    expect(await screen.findByText("vs punt_god · 122.4 – 108.9")).toBeInTheDocument();
    expect(h.api.loadSeason).toHaveBeenCalledWith(false);
    // Playoff odds come through as a percentage, not a raw fraction — once in
    // the header strip and once in the standings row.
    expect(screen.getAllByText("88%")).toHaveLength(2);
  });

  it("drives the league tabs from the keyboard, as the tablist role promises", async () => {
    const user = userEvent.setup();
    const initial = fixture();
    h.api.getConfig.mockResolvedValue(restoringConfig(initial));
    h.api.addLeague.mockResolvedValue(initial);
    h.api.loadSeason.mockResolvedValue(seasonFixture());

    render(<App />);
    await screen.findByRole("heading", { name: initial.league.name });
    await user.click(screen.getByRole("button", { name: "Season" }));
    await screen.findByText("vs punt_god · 122.4 – 108.9");

    const standings = screen.getByRole("tab", { name: "Standings" });
    expect(standings).toHaveAttribute("aria-selected", "true");
    // Only the selected tab is in the tab order.
    expect(standings).toHaveAttribute("tabindex", "0");
    expect(screen.getByRole("tab", { name: "Games" })).toHaveAttribute("tabindex", "-1");

    standings.focus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "Games" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "Games" })).toHaveFocus();

    await user.keyboard("{End}");
    expect(screen.getByRole("tab", { name: "Last season" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    // Wraps around rather than stopping at the end.
    await user.keyboard("{ArrowRight}");
    expect(standings).toHaveAttribute("aria-selected", "true");

    // The panel is announced and points back at its tab.
    const panel = screen.getByRole("tabpanel");
    expect(panel).toHaveAttribute("aria-labelledby", standings.id);
    expect(standings).toHaveAttribute("aria-controls", panel.id);
  });

  it("shows a season load failure as an error with a working retry", async () => {
    const user = userEvent.setup();
    const initial = fixture();
    h.api.getConfig.mockResolvedValue(restoringConfig(initial));
    h.api.addLeague.mockResolvedValue(initial);
    h.api.loadSeason
      .mockRejectedValueOnce(new Error("Sleeper timed out"))
      .mockResolvedValueOnce(seasonFixture());

    render(<App />);
    await screen.findByRole("heading", { name: initial.league.name });
    await user.click(screen.getByRole("button", { name: "Season" }));

    const retry = await screen.findByRole("button", { name: "Try again" });
    // Once in the toast, once in the error block itself.
    expect(screen.getAllByText(/Sleeper timed out/)).toHaveLength(2);

    await user.click(retry);
    expect(await screen.findByText("vs punt_god · 122.4 – 108.9")).toBeInTheDocument();
    expect(h.api.loadSeason).toHaveBeenLastCalledWith(true);
  });
});

describe("when an action fails", () => {
  /** The app loaded on the draft board, ready to take a pick. */
  async function loadedOnTheBoard(initial: DraftView) {
    h.api.getConfig.mockResolvedValue(restoringConfig(initial));
    h.api.addLeague.mockResolvedValue(initial);
    render(<App />);
    await screen.findByRole("heading", { name: initial.league.name });
    const rows = await rowDraftButtons();
    return rows[rows.length - 1];
  }

  it("says so in plain words, waits to be answered, and tries again on request", async () => {
    const initial = fixture();
    const afterPick = fixture();
    afterPick.available = afterPick.available.slice(1);
    h.api.recordManualPick
      .mockRejectedValueOnce(new Error("Sleeper is not answering"))
      .mockResolvedValueOnce(afterPick);

    const row = await loadedOnTheBoard(initial);
    // Fake timers only once the app is up, so loading is not held back by them.
    vi.useFakeTimers();

    await settle(() => {
      row.click();
    });
    await settle(() => {
      within(screen.getByRole("dialog")).getByRole("button", { name: "Mark drafted" }).click();
    });

    // An error is announced straight away, not filed away politely.
    const failure = screen.getByRole("alert");
    expect(failure).toHaveTextContent(/Could not mark .+ as drafted — Sleeper is not answering/);

    // Long past the five seconds an informational toast lives for.
    act(() => {
      vi.advanceTimersByTime(30_000);
    });
    expect(screen.getByRole("alert")).toBeInTheDocument();

    await settle(() => {
      screen.getByRole("button", { name: "Try again" }).click();
    });
    expect(h.api.recordManualPick).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("still lets an informational message get out of the way on its own", async () => {
    const initial = fixture();
    const refreshed = fixture();
    refreshed.data_health.board_size = 312;
    h.api.refreshData.mockResolvedValue(refreshed);

    await loadedOnTheBoard(initial);
    vi.useFakeTimers();

    await settle(() => {
      screen.getByRole("button", { name: "Settings" }).click();
    });
    await settle(() => {
      screen.getByRole("menuitemcheckbox", { name: /Refresh data/ }).click();
    });

    const note = "Projections refreshed — board rebuilt from 312 players";
    expect(screen.getByText(note)).toBeInTheDocument();
    // Nothing to decide, so it is announced politely and clears itself.
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(5000);
    });
    expect(screen.queryByText(note)).not.toBeInTheDocument();
  });
});
