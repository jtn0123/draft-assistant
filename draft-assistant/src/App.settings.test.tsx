// Grade item D6. The settings menu is where most of the shell's behaviour
// lives — six rows, each one an action against the backend that can fail —
// and almost none of it was reached by a test. Everything here is asserted
// through what a user sees on the row or in the message underneath.

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./api", async () => ({ api: (await import("./test/appHarness")).harness().api }));

import "./test/warmScreens";
import App from "./App";
import { setAvatarMode } from "./avatars";
import { resetPrefs } from "./prefs";
import { resetThemePreference } from "./theme";
import { settle } from "./test/settle";
import { settingsRow } from "./test/settingsRow";
import { draftFixture, fakeStorage, harness, restoringConfig } from "./test/appHarness";
import type { DraftView } from "./types";

const h = harness();

/**
 * Load the app on the draft board with a league already saved.
 *
 * Waits on the header's heading, not on the league name as text: the launch
 * card names the league it is restoring too, in a <strong>, and under load a
 * text wait can resolve on that card before the view — and the chime — lands.
 * Then settles: the heading is in the DOM the moment React commits, but the
 * effects that commit schedules (the chime among them) run in a later task,
 * and a busy machine can let the wait's own timer fire ahead of it.
 */
async function loaded(view = draftFixture()) {
  h.api.getConfig.mockResolvedValue(restoringConfig(view));
  h.api.addLeague.mockResolvedValue(view);
  render(<App />);
  await screen.findByRole("heading", { name: view.league.name });
  await settle(() => {});
}

/** Open the settings menu and click one of its rows. */
async function chooseSetting(label: RegExp) {
  await settle(() => {
    const gear = screen.queryByRole("button", { name: "Settings" });
    if (gear !== null && screen.queryByRole("menu") === null) gear.click();
  });
  await settle(() => {
    settingsRow(label).click();
  });
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
    expect(screen.getByText("Live sync on: polling Sleeper every 3s")).toBeInTheDocument();
    // Once polling, the row reports how long ago the last sync landed.
    expect(settingsRow(/Live sync/)).toHaveTextContent(/Last sync/);
  });

  it("says so when live sync cannot be turned off, and offers another go", async () => {
    h.api.stopPolling.mockRejectedValue(new Error("backend is not listening"));
    await loaded();
    await chooseSetting(/Live sync/);

    const failure = screen.getByRole("alert");
    expect(failure).toHaveTextContent("Could not change live sync: backend is not listening");
    await settle(() => {
      screen.getByRole("button", { name: "Try again" }).click();
    });
    expect(h.api.stopPolling).toHaveBeenCalledTimes(2);
  });

  it("says so when live sync could not be started at launch", async () => {
    h.api.startPolling.mockRejectedValueOnce(new Error("no draft to poll"));
    await loaded();

    const failure = await screen.findByRole("alert");
    expect(failure).toHaveTextContent("Could not turn live sync on: no draft to poll");
    await settle(() => {
      screen.getByRole("button", { name: "Try again" }).click();
    });
    expect(h.api.startPolling).toHaveBeenCalledTimes(2);
  });
});

describe("the Sleeper username row", () => {
  it("opens the setup screen, where the username lives, with a way back", async () => {
    // The roster panel tells a Sleeper user to set their username, and until
    // now nothing after the first launch could reach the field.
    await loaded();
    await chooseSetting(/Sleeper username/);
    expect(await screen.findByLabelText("Sleeper username")).toBeInTheDocument();
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Back to/ })).toBeInTheDocument();
  });

  it("saves a username typed on its own and comes back to the board", async () => {
    // Arriving was not the point. The form's submit needed a league id, so a
    // user who came to change one word was stuck until they went and found
    // the id of the league already on their screen.
    const view = draftFixture();
    await loaded(view);
    h.api.setMyUsername.mockResolvedValue("mcsleeper26");
    await chooseSetting(/Sleeper username/);

    const field = await screen.findByLabelText("Sleeper username");
    expect(screen.getByLabelText("League ID")).toHaveValue(view.league.league_id);
    await settle(() => {
      fireEvent.change(field, { target: { value: "mcsleeper26" } });
    });
    await settle(() => {
      fireEvent.submit(field.closest("form") as HTMLFormElement);
    });

    await waitFor(() => expect(h.api.setMyUsername).toHaveBeenCalledWith("mcsleeper26"));
    expect(h.api.addLeague).toHaveBeenLastCalledWith(view.league.league_id);
    expect(await screen.findByRole("heading", { name: view.league.name })).toBeInTheDocument();
    expect(screen.queryByLabelText("Sleeper username")).toBeNull();
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
      "Could not refresh the projections: projections are down",
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
      "Could not undo the last recorded pick: nothing to undo",
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
    expect(row).toHaveTextContent("Team logos only, no photo downloads");
    expect(row).toHaveTextContent("Team logos");
    expect(row).toHaveAttribute("aria-checked", "false");

    await settle(() => {
      settingsRow(/Player pictures/).click();
    });
    expect(settingsRow(/Player pictures/)).toHaveTextContent(/Headshots from Sleeper/);
    expect(settingsRow(/Player pictures/)).toHaveAttribute("aria-checked", "true");
  });
});

describe("the appearance picker", () => {
  it("moves between following the system and an explicit light or dark", async () => {
    await loaded();
    await settle(() => {
      screen.getByRole("button", { name: "Settings" }).click();
    });

    const choice = (name: string) => screen.getByRole("menuitemradio", { name });
    const group = () => screen.getByRole("group", { name: "Appearance" });
    expect(group().parentElement).toHaveTextContent(
      "Following your system setting, light right now",
    );
    expect(choice("System")).toHaveAttribute("aria-checked", "true");

    // Dark is two steps from System on the shell's cycle; one click gets there.
    await settle(() => {
      choice("Dark").click();
    });
    expect(choice("Dark")).toHaveAttribute("aria-checked", "true");
    expect(choice("System")).toHaveAttribute("aria-checked", "false");
    expect(group().parentElement).toHaveTextContent("Overriding your system setting");
    expect(document.documentElement.dataset.theme).toBe("dark");

    await settle(() => {
      choice("Light").click();
    });
    expect(choice("Light")).toHaveAttribute("aria-checked", "true");
    expect(document.documentElement.dataset.theme).toBe("light");

    await settle(() => {
      choice("System").click();
    });
    expect(choice("System")).toHaveAttribute("aria-checked", "true");
    expect(group().parentElement).toHaveTextContent("Following your system setting");
  });
});

describe("the league row", () => {
  /** The saved config, with a second league already loaded once. */
  function twoLeagues(view: DraftView) {
    return {
      ...restoringConfig(view),
      leagues: [
        { league_id: view.league.league_id, name: view.league.name, season: "2026", status: null },
        { league_id: "2222222222222222222", name: "Mock draft", season: "2026", status: null },
      ],
    };
  }

  it("opens the picker with every league the app has loaded", async () => {
    const view = draftFixture();
    h.api.getConfig.mockResolvedValue(twoLeagues(view));
    h.api.addLeague.mockResolvedValue(view);
    render(<App />);
    await screen.findByRole("heading", { name: view.league.name });

    await chooseSetting(/League/);
    expect(screen.getByRole("dialog", { name: "Switch league" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Mock draft/ })).toBeInTheDocument();
  });

  it("stops the old poller, loads the new league, and restarts live sync", async () => {
    const view = draftFixture();
    const other = draftFixture();
    other.league = { ...other.league, league_id: "2222222222222222222", name: "Mock draft" };
    h.api.getConfig.mockResolvedValue(twoLeagues(view));
    h.api.addLeague.mockResolvedValue(view);
    render(<App />);
    await screen.findByRole("heading", { name: view.league.name });
    await waitFor(() => expect(h.api.startPolling).toHaveBeenCalledTimes(1));

    h.api.addLeague.mockResolvedValue(other);
    await chooseSetting(/League/);
    await settle(() => {
      screen.getByRole("button", { name: /^Mock draft/ }).click();
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
    await screen.findByRole("heading", { name: view.league.name });

    h.api.addLeague.mockRejectedValue(new Error("league 2222222222222222222 not found"));
    await chooseSetting(/League/);
    await settle(() => {
      screen.getByRole("button", { name: /^Mock draft/ }).click();
    });

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Could not switch leagues");
    expect(alert).toHaveTextContent("not found");
    // The league that was on screen is still the one on screen.
    expect(screen.getByText(view.league.name)).toBeInTheDocument();
  });

  it("opens from the league name in the header, and forgets a league from its row", async () => {
    const view = draftFixture();
    const config = twoLeagues(view);
    h.api.getConfig.mockResolvedValue(config);
    h.api.addLeague.mockResolvedValue(view);
    h.api.removeLeague.mockResolvedValue([config.leagues[0]]);
    render(<App />);
    await screen.findByRole("heading", { name: view.league.name });

    await settle(() => screen.getByRole("button", { name: view.league.name }).click());
    expect(screen.getByRole("dialog", { name: "Switch league" })).toBeInTheDocument();
    // The account is saved, so Sleeper was asked without a click.
    expect(h.api.sleeperLeagues).toHaveBeenCalledWith(view.league.season);

    await settle(() => screen.getByRole("button", { name: "Forget Mock draft" }).click());
    expect(h.api.removeLeague).toHaveBeenCalledWith("2222222222222222222");
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: /^Mock draft/ })).not.toBeInTheDocument(),
    );
    expect(h.api.addLeague).toHaveBeenCalledTimes(1);
  });
});
