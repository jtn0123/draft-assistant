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
  await settle(() => {
    const gear = screen.queryByRole("button", { name: "Settings" });
    if (gear !== null && screen.queryByRole("menu") === null) gear.click();
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
});

describe("the Yahoo settings row", () => {
  it("says nothing is connected and offers to connect", async () => {
    await loaded();
    await openSettings();
    const row = settingsRow(/Yahoo/);
    expect(row).toHaveTextContent("Not connected");
    expect(row).toHaveTextContent("Connect");
    expect(row).toHaveAttribute("aria-checked", "false");
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
    expect(row).toHaveAttribute("aria-checked", "true");
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

  it("follows the dialog's own answer without asking the backend again", async () => {
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
    await settle(() => screen.getByRole("button", { name: "Close" }).click());
    await openSettings();
    expect(settingsRow(/Yahoo/)).toHaveTextContent("Connected as jtn0123");
  });
});

describe("the attribution line", () => {
  it("credits Yahoo under the menu when the league is a Yahoo one", async () => {
    const view = draftFixture();
    view.league.platform = "yahoo";
    await loaded(view);
    await openSettings();

    expect(
      screen.getByText("Fantasy data provided by Yahoo Fantasy · read-only connection"),
    ).toBeInTheDocument();
  });

  it("names the Sleeper league instead when it is one of theirs", async () => {
    const view = draftFixture();
    view.league.platform = "sleeper";
    await loaded(view);
    await openSettings();

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
