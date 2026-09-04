// The shell around the screens: the header's re-pull button, the version row,
// and the service the live-sync row names. Split out of App.settings.test.tsx
// when that file reached the 500-line cap.

import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./api", async () => ({ api: (await import("./test/appHarness")).harness().api }));
// The shell that knows the running version. Outside Tauri it is not there at
// all, which is the browser preview's case and the default here.
vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn() }));

import "./test/warmScreens";
import App from "./App";
import { getVersion } from "@tauri-apps/api/app";
import { resetPrefs } from "./prefs";
import { resetThemePreference } from "./theme";
import { settle } from "./test/settle";
import { draftFixture, fakeStorage, harness, restoringConfig } from "./test/appHarness";

const h = harness();

/** Load the app on the draft board with a league already saved. */
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
    screen.getByRole("menuitemcheckbox", { name: label }).click();
  });
}

function settingsRow(label: RegExp): HTMLElement {
  return screen.getByRole("menuitemcheckbox", { name: label });
}

beforeEach(() => {
  h.reset();
  vi.mocked(getVersion).mockRejectedValue(new Error("not running in Tauri"));
  fakeStorage({ "da.screen": "draft" });
  resetPrefs();
  resetThemePreference();
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("the live sync row on a Yahoo league", () => {
  it("names Yahoo rather than telling a Yahoo player about Sleeper", async () => {
    const view = draftFixture();
    view.league.platform = "yahoo";
    view.league.league_id = "449.l.12345";
    await loaded(view);

    await chooseSetting(/Live sync/);
    expect(settingsRow(/Live sync/)).toHaveTextContent("Not polling Yahoo");

    await settle(() => {
      settingsRow(/Live sync/).click();
    });
    expect(screen.getByText("Live sync on — polling Yahoo every 3s")).toBeInTheDocument();
  });
});

describe("the version row", () => {
  it("shows the version the shell reports", async () => {
    vi.mocked(getVersion).mockResolvedValue("9.9.9");
    await loaded();

    await settle(() => {
      screen.getByRole("button", { name: "Settings" }).click();
    });
    await waitFor(() => expect(settingsRow(/Version/)).toHaveTextContent("v9.9.9"));
  });

  it("falls back to the packaged version where there is no shell to ask", async () => {
    await loaded();

    await settle(() => {
      screen.getByRole("button", { name: "Settings" }).click();
    });
    // The browser preview: `getVersion` throws, and the row still says
    // something rather than "vundefined".
    expect(settingsRow(/Version/)).toHaveTextContent(/^Versionv?/);
    expect(settingsRow(/Version/)).toHaveTextContent(/v\d+\.\d+\.\d+/);
  });
});

describe("re-pulling the picks", () => {
  it("asks the backend for the picks again and says what came back", async () => {
    const view = draftFixture();
    h.api.refreshPicks.mockResolvedValue(view);
    await loaded(view);

    await settle(() => {
      screen.getByRole("button", { name: "Re-pull picks" }).click();
    });

    expect(h.api.refreshPicks).toHaveBeenCalledTimes(1);
    expect(screen.getByText(/^Picks re-pulled from Sleeper/)).toBeInTheDocument();
  });

  it("offers the failure again rather than leaving the board looking fresh", async () => {
    h.api.refreshPicks.mockRejectedValue(new Error("network timeout"));
    await loaded();

    await settle(() => {
      screen.getByRole("button", { name: "Re-pull picks" }).click();
    });

    const failure = screen.getByRole("alert");
    expect(failure).toHaveTextContent("Could not re-pull the picks — network timeout");
    await settle(() => {
      screen.getByRole("button", { name: "Try again" }).click();
    });
    expect(h.api.refreshPicks).toHaveBeenCalledTimes(2);
  });
});
