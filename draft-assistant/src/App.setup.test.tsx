// The first launch, before there is a league — and what a Yahoo player can do
// with it.
//
// The setup screen used to be Sleeper and nothing else: a username and a
// league id, with the Yahoo dialog reachable only from a settings menu that
// does not exist until a league is on screen. A Yahoo-only player had no way
// in at all.

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./api", async () => ({ api: (await import("./test/appHarness")).harness().api }));

import "./test/warmScreens";
import App from "./App";
import { resetPrefs } from "./prefs";
import { resetThemePreference } from "./theme";
import { settle } from "./test/settle";
import { draftFixture, fakeStorage, harness } from "./test/appHarness";
import type { StoredLeague } from "./types";

const h = harness();

const YAHOO_LEAGUE: StoredLeague = {
  league_id: "449.l.12345",
  name: "Sunday Money",
  season: "2026",
  status: "pre_draft",
  platform: "yahoo",
};

/** Launch with nothing saved, so the setup screen is what comes up. */
async function firstLaunch() {
  h.api.getConfig.mockResolvedValue({
    my_user_id: null,
    active_league_id: null,
    leagues: [],
  });
  render(<App />);
  await screen.findByRole("button", { name: "Connect Yahoo instead" });
  await settle(() => {});
}

beforeEach(() => {
  h.reset();
  fakeStorage({ "da.screen": "draft" });
  resetPrefs();
  resetThemePreference();
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("the setup screen", () => {
  it("offers Yahoo as a way in, not just a Sleeper league id", async () => {
    await firstLaunch();

    expect(screen.getByRole("button", { name: "Load league" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Connect Yahoo instead" })).toBeInTheDocument();
  });

  it("opens the Yahoo dialog and loads the league picked in it", async () => {
    const view = draftFixture();
    view.league.platform = "yahoo";
    view.league.league_id = YAHOO_LEAGUE.league_id;
    view.league.name = YAHOO_LEAGUE.name;
    h.api.yahooStatus.mockResolvedValue({
      configured: true,
      connected: true,
      redirect: "oob",
      account: "yahoo-player",
    });
    h.api.yahooLeagues.mockResolvedValue([YAHOO_LEAGUE]);
    h.api.addLeague.mockResolvedValue(view);
    await firstLaunch();

    await userEvent.click(screen.getByRole("button", { name: "Connect Yahoo instead" }));
    await userEvent.click(screen.getByRole("button", { name: /Find my Yahoo leagues/ }));
    await screen.findByText("Sunday Money");
    await userEvent.click(screen.getByRole("button", { name: /Sunday Money/ }));

    // The league the dialog handed back is loaded and live, and the setup
    // screen is behind us.
    await waitFor(() => expect(h.api.addLeague).toHaveBeenCalledWith(YAHOO_LEAGUE.league_id));
    await screen.findByRole("heading", { name: "Sunday Money" });
    await waitFor(() => expect(h.api.startPolling).toHaveBeenCalled());
  });

  it("says why a league from the Yahoo dialog would not load", async () => {
    h.api.yahooStatus.mockResolvedValue({
      configured: true,
      connected: true,
      redirect: "oob",
      account: "yahoo-player",
    });
    h.api.yahooLeagues.mockResolvedValue([YAHOO_LEAGUE]);
    h.api.addLeague.mockRejectedValue(new Error("yahoo said no"));
    await firstLaunch();

    await userEvent.click(screen.getByRole("button", { name: "Connect Yahoo instead" }));
    await userEvent.click(screen.getByRole("button", { name: /Find my Yahoo leagues/ }));
    await screen.findByText("Sunday Money");
    await userEvent.click(screen.getByRole("button", { name: /Sunday Money/ }));

    // Still on the setup screen, with the failure said out loud rather than
    // swallowed by a dialog that has closed.
    expect(await screen.findByText(/yahoo said no/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Connect Yahoo instead" })).toBeInTheDocument();
  });
});

describe("the launch screen", () => {
  it("names the service it is actually waiting on", async () => {
    h.api.getConfig.mockResolvedValue({
      my_user_id: null,
      active_league_id: YAHOO_LEAGUE.league_id,
      leagues: [YAHOO_LEAGUE],
    });
    // Never resolves: the launch card is what stays on screen.
    h.api.addLeague.mockReturnValue(new Promise(() => undefined));
    render(<App />);

    expect(await screen.findByText("Connecting to Yahoo")).toBeInTheDocument();
  });
});
