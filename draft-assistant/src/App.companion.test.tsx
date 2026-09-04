// The shell in both modes: a host with the companion rows, and a follower
// with the pill, the trimmed menu, and no way to change the host's mind.

import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./api", async () => ({ api: (await import("./test/appHarness")).harness().api }));

import "./test/warmScreens";
import App from "./App";
import { resetPrefs } from "./prefs";
import { resetThemePreference } from "./theme";
import { settle } from "./test/settle";
import {
  companionStatus,
  draftFixture,
  fakeStorage,
  harness,
  restoringConfig,
} from "./test/appHarness";

const h = harness();

const FOLLOW = JSON.stringify({
  url: "http://192.168.1.5:7878",
  token: "tok-1",
  host_name: "Justin's Mac",
});

async function loaded() {
  const view = draftFixture();
  h.api.getConfig.mockResolvedValue(restoringConfig(view));
  h.api.addLeague.mockResolvedValue(view);
  render(<App />);
  await screen.findByRole("heading", { name: view.league.name });
  await settle(() => {});
}

async function openSettings() {
  await settle(() => {
    const gear = screen.queryByRole("button", { name: "Settings" });
    if (gear !== null && screen.queryByRole("menu") === null) gear.click();
  });
}

const row = (label: RegExp) => screen.queryByRole("menuitemcheckbox", { name: label });

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

describe("hosting", () => {
  it("offers both companion rows and opens the dialog", async () => {
    await loaded();
    await openSettings();

    expect(row(/Phone & second screen/)).toBeInTheDocument();
    expect(row(/Join another Draft Assistant/)).toBeInTheDocument();
    expect(row(/Leave host/)).toBeNull();

    await settle(() => row(/Phone & second screen/)?.click());
    expect(screen.getByRole("dialog", { name: /Phone & second screen/ })).toBeInTheDocument();
  });

  it("says on the row when the server is already up", async () => {
    h.api.companionStatus.mockResolvedValue(companionStatus({ enabled: true }));
    await loaded();
    await openSettings();
    expect(row(/Phone & second screen/)).toHaveTextContent("On");
  });

  it("opens the join dialog from the settings menu", async () => {
    await loaded();
    await openSettings();
    await settle(() => row(/Join another Draft Assistant/)?.click());
    expect(
      screen.getByRole("dialog", { name: "Join another Draft Assistant" }),
    ).toBeInTheDocument();
  });
});

describe("following a host", () => {
  beforeEach(() => {
    fakeStorage({ "da.screen": "draft", "da.companion.follow": FOLLOW });
  });

  it("wears the host's name in the header", async () => {
    await loaded();
    expect(screen.getByText("Hosted by Justin's Mac")).toBeInTheDocument();
  });

  it("hides everything the host owns and offers a way out", async () => {
    await loaded();
    await openSettings();

    expect(row(/Leave host/)).toBeInTheDocument();
    expect(row(/Yahoo/)).toBeNull();
    expect(row(/^League/)).toBeNull();
    expect(row(/Export state/)).toBeNull();
    expect(row(/Import projections/)).toBeNull();
    expect(row(/Refresh data/)).toBeNull();
    expect(row(/Join another Draft Assistant/)).toBeNull();
    // The rows that are about this Mac stay.
    expect(row(/Pick chime/)).toBeInTheDocument();
    expect(row(/Appearance/)).toBeInTheDocument();
  });

  it("sends nobody to the league picker — the host chooses", async () => {
    await loaded();
    await settle(() => screen.getByTitle("Switch league").click());
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByText("Justin's Mac picks the league")).toBeInTheDocument();
  });

  it("keeps the host's own controls off the header", async () => {
    await loaded();
    expect(screen.queryByRole("button", { name: /Re-pull picks/ })).toBeNull();
    expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
  });

  it("never asks the host's app whether it is serving phones", async () => {
    await loaded();
    expect(h.api.companionStatus).not.toHaveBeenCalled();
  });
});

describe("being dropped", () => {
  it("says so on the way back into local mode", async () => {
    fakeStorage({ "da.screen": "draft", "da.companion.revoked": "1" });
    await loaded();
    expect(screen.getByText("The host revoked this device")).toBeInTheDocument();
    // Read once: a reload after this should not repeat it.
    expect(localStorage.getItem("da.companion.revoked")).toBeNull();
  });
});
