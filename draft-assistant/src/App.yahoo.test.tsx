// Yahoo, as the shell shows it: the settings row that says where the
// connection stands and opens the dialog, the attribution line Yahoo's terms
// ask for, and the connected status reaching the league picker.

import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./api", async () => ({ api: (await import("./test/appHarness")).harness().api }));

import "./test/warmScreens";
import App from "./App";
import { resetPrefs } from "./prefs";
import { resetThemePreference } from "./theme";
import { settle } from "./test/settle";
import { settingsRow, openSettingsPage } from "./test/settingsRow";
import { draftFixture, fakeStorage, harness, restoringConfig } from "./test/appHarness";
import type { DraftView } from "./types";

const h = harness();

async function loaded(view: DraftView = draftFixture()) {
  h.api.getConfig.mockResolvedValue(restoringConfig(view));
  h.api.addLeague.mockResolvedValue(view);
  render(<App />);
  await screen.findByRole("heading", { name: view.league.name });
  await settle(() => {});
}

/** Open the settings menu, if it is not already open. */
async function openSettings() {
  await openSettingsPage();
}

beforeEach(() => {
  h.reset();
  fakeStorage({ "da.screen": "draft" });
  resetPrefs();
  resetThemePreference();
});

describe("the Yahoo settings row", () => {
  it("says nothing is connected and offers to connect", async () => {
    await loaded();
    await openSettings();
    const row = settingsRow(/Yahoo/);
    expect(row).toHaveTextContent("Not connected");
    expect(row).toHaveTextContent("Connect");
    // An action that opens the connect dialog, so no checked state to
    // announce; connected or not shows on the value.
    expect(row).not.toHaveAttribute("aria-checked");
    expect(row.querySelector(".settings-row-value")).not.toHaveClass("is-on");
  });

  it("names the account once Yahoo is connected", async () => {
    h.api.yahooStatus.mockResolvedValue({
      configured: true,
      connected: true,
      redirect: "oob",
      account: "jtn0123",
    });
    await loaded();
    await openSettings();
    const row = settingsRow(/Yahoo/);
    expect(row).toHaveTextContent("Connected as jtn0123");
    expect(row.querySelector(".settings-row-value")).toHaveClass("is-on");
  });

  it("says nothing is connected when the status could not be read at all", async () => {
    // Not worth a toast: not knowing and not being connected look the same
    // from here, and the dialog reports properly when it is opened.
    h.api.yahooStatus.mockRejectedValue(new Error("keychain is locked"));
    await loaded();
    await openSettings();
    expect(settingsRow(/Yahoo/)).toHaveTextContent("Not connected");
    expect(screen.queryByText("keychain is locked")).toBeNull();
  });

  it("opens the connect dialog, closing the menu behind it", async () => {
    await loaded();
    await openSettings();
    await settle(() => settingsRow(/Yahoo/).click());

    expect(screen.getByRole("dialog", { name: "Connect Yahoo Fantasy" })).toBeInTheDocument();
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("follows the dialog's own answer while it is open", async () => {
    await loaded();
    expect(h.api.yahooStatus).toHaveBeenCalledTimes(1);
    // The dialog asks again as it opens, and hands what it hears back up.
    h.api.yahooStatus.mockResolvedValue({
      configured: true,
      connected: true,
      redirect: "oob",
      account: "jtn0123",
    });
    await openSettings();
    await settle(() => settingsRow(/Yahoo/).click());
    expect(h.api.yahooStatus).toHaveBeenCalledTimes(2);
    await settle(() => screen.getByRole("button", { name: "Close" }).click());
    await openSettings();
    expect(settingsRow(/Yahoo/)).toHaveTextContent("Connected as jtn0123");
  });

  it("asks again as the dialog closes, so a sign-in the browser finished reaches the row", async () => {
    // The failure this prevents: with a loopback redirect the backend
    // finishes the sign-in after the browser comes back, with nothing in
    // the dialog to hand a status up. The user closed the dialog as told
    // and the row went on saying "Not connected" over a connected account.
    await loaded();
    await openSettings();
    await settle(() => settingsRow(/Yahoo/).click());
    expect(screen.getByRole("dialog", { name: "Connect Yahoo Fantasy" })).toBeInTheDocument();
    // The sign-in finishes in the backend while the dialog is open.
    h.api.yahooStatus.mockResolvedValue({
      configured: true,
      connected: true,
      redirect: "http://localhost:8731/",
      account: "jtn0123",
    });
    await settle(() => screen.getByRole("button", { name: "Close" }).click());
    await openSettings();
    expect(settingsRow(/Yahoo/)).toHaveTextContent("Connected as jtn0123");
  });

  it("asks again when the poller says Yahoo signed the user out", async () => {
    // The failure this prevents: the backend cleared a pair Yahoo refused
    // mid-draft, and with no dialog open to notice, the row went on saying
    // "Connected as ..." until the app was restarted.
    h.api.yahooStatus.mockResolvedValue({
      configured: true,
      connected: true,
      redirect: "oob",
      account: "jtn0123",
    });
    await loaded();
    await openSettings();
    expect(settingsRow(/Yahoo/)).toHaveTextContent("Connected as jtn0123");

    h.api.yahooStatus.mockResolvedValue({
      configured: true,
      connected: false,
      redirect: "oob",
      account: null,
    });
    await settle(() =>
      h.push.health?.({
        last_success_at: 1788452521,
        consecutive_failures: 1,
        last_error: "Yahoo signed you out. Connect again in Settings.",
      }),
    );
    expect(settingsRow(/Yahoo/)).toHaveTextContent("Not connected");
    // Any other poll failure is not a reason to ask.
    const asked = h.api.yahooStatus.mock.calls.length;
    await settle(() =>
      h.push.health?.({
        last_success_at: 1788452521,
        consecutive_failures: 2,
        last_error: "request failed: timed out",
      }),
    );
    expect(h.api.yahooStatus).toHaveBeenCalledTimes(asked);
  });
});

describe("the attribution line", () => {
  it("credits Yahoo under the menu when the league is a Yahoo one", async () => {
    const view = draftFixture();
    view.league.platform = "yahoo";
    await loaded(view);
    await settle(() => screen.getByRole("button", { name: "Settings" }).click());

    expect(
      screen.getByText("Fantasy data provided by Yahoo Fantasy · read-only connection"),
    ).toBeInTheDocument();
  });

  it("names the Sleeper league instead when it is one of theirs", async () => {
    const view = draftFixture();
    view.league.platform = "sleeper";
    await loaded(view);
    await settle(() => screen.getByRole("button", { name: "Settings" }).click());

    expect(screen.getByText(new RegExp(`league ${view.league.league_id}`))).toBeInTheDocument();
    expect(screen.queryByText(/provided by Yahoo Fantasy/)).toBeNull();
  });
});

describe("the picker's Yahoo lookup", () => {
  it("leaves Yahoo alone while nothing is connected", async () => {
    await loaded();
    await openSettings();
    await settle(() => settingsRow(/League/).click());

    expect(screen.getByRole("dialog", { name: "Switch league" })).toBeInTheDocument();
    expect(h.api.yahooLeagues).not.toHaveBeenCalled();
  });

  it("merges the Yahoo account's leagues in once it is connected", async () => {
    h.api.yahooStatus.mockResolvedValue({
      configured: true,
      connected: true,
      redirect: "oob",
      account: "jtn0123",
    });
    h.api.yahooLeagues.mockResolvedValue([
      {
        league_id: "449.l.98765",
        name: "Office League",
        season: "2026",
        status: "pre_draft",
        platform: "yahoo",
      },
    ]);
    await loaded();
    await openSettings();
    await settle(() => settingsRow(/League/).click());

    expect(h.api.yahooLeagues).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: /Office League/ })).toHaveTextContent("Yahoo");
  });
});
