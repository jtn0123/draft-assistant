// The Settings row "Import projections CSV…": what it calls, what it says
// afterwards, and what it does when the user closes the picker or the file
// turns out not to be a projections export.

import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./api", async () => ({ api: (await import("./test/appHarness")).harness().api }));

import "./test/warmScreens";
import App from "./App";
import { setAvatarMode } from "./avatars";
import { resetPrefs } from "./prefs";
import { resetThemePreference } from "./theme";
import { settle } from "./test/settle";
import { draftFixture, fakeStorage, harness, restoringConfig } from "./test/appHarness";
import type { DraftView } from "./types";

const h = harness();

async function loaded(view: DraftView) {
  h.api.getConfig.mockResolvedValue(restoringConfig(view));
  h.api.addLeague.mockResolvedValue(view);
  render(<App />);
  await screen.findByText(view.league.name);
}

async function chooseSetting(label: RegExp) {
  await settle(() => {
    const gear = screen.queryByRole("button", { name: "Settings" });
    if (gear !== null && screen.queryByRole("menu") === null) gear.click();
  });
  await settle(() => {
    screen.getByRole("menuitemcheckbox", { name: label }).click();
  });
}

/** The fixture with nothing imported: no load date, no player opinions. */
function withoutImport(): DraftView {
  const view = draftFixture(3);
  view.data_health.second_opinion_loaded_at = null;
  view.available = view.available.map((p) => ({ ...p, second_opinion: null }));
  return view;
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

describe("the import projections row", () => {
  it("offers the import when nothing has been loaded", async () => {
    await loaded(withoutImport());
    await settle(() => {
      screen.queryByRole("button", { name: "Settings" })?.click();
    });
    const row = screen.getByRole("menuitemcheckbox", { name: /Import projections CSV/ });
    expect(row).toHaveTextContent("Add a second opinion column to the board");
    expect(row).toHaveAttribute("aria-checked", "false");
  });

  it("names the source and the load date once something is loaded", async () => {
    await loaded(draftFixture(3));
    await settle(() => {
      screen.queryByRole("button", { name: "Settings" })?.click();
    });
    const row = screen.getByRole("menuitemcheckbox", { name: /Import projections CSV/ });
    expect(row).toHaveTextContent(/Clay loaded/);
    expect(row).toHaveAttribute("aria-checked", "true");
  });

  it("imports the chosen file and reports the match counts", async () => {
    const imported = draftFixture(3);
    await loaded(withoutImport());
    h.api.importSecondOpinion.mockResolvedValue({
      matched: 418,
      total: 482,
      message: "Second opinion loaded: 418 of 482 players matched",
      view: imported,
    });

    await chooseSetting(/Import projections CSV/);

    expect(h.api.importSecondOpinion).toHaveBeenCalledTimes(1);
    expect(
      await screen.findByText("Second opinion loaded: 418 of 482 players matched"),
    ).toBeInTheDocument();
  });

  it("says nothing at all when the user closes the picker", async () => {
    await loaded(withoutImport());
    h.api.importSecondOpinion.mockResolvedValue(null);

    await chooseSetting(/Import projections CSV/);

    expect(h.api.importSecondOpinion).toHaveBeenCalledTimes(1);
    expect(screen.queryByText(/Second opinion loaded/)).toBeNull();
    expect(screen.queryByText(/Could not import/)).toBeNull();
  });

  it("shows the backend's plain-words complaint about a file it cannot read", async () => {
    await loaded(withoutImport());
    h.api.importSecondOpinion.mockRejectedValue(
      new Error('that file has no "name" column, so it is not a projections export'),
    );

    await chooseSetting(/Import projections CSV/);

    expect(await screen.findByText(/Could not import those projections/)).toBeInTheDocument();
    expect(screen.getByText(/no "name" column/)).toBeInTheDocument();
    // Something that failed offers another go rather than vanishing.
    expect(screen.getByRole("button", { name: "Try again" })).toBeInTheDocument();
  });
});
