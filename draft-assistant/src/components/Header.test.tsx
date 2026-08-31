import { useState } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PollHealth } from "../types";
import { Header, type SettingsRow } from "./Header";
import { resetPrefs, setChime } from "../prefs";
import { settle } from "../test/settle";

const rows = (onSelect = () => {}): SettingsRow[] => [
  {
    label: "Pick chime",
    note: "Sound when you're on the clock",
    value: "On",
    on: true,
    onSelect,
  },
  { label: "Live sync", note: "Not polling Sleeper", value: "Off", on: false, onSelect },
  { label: "Export state", note: "Full JSON dump", value: "JSON", on: false, onSelect },
];

/** The header with the settings menu wired up the way the app wires it. */
function Harness({
  onSelect,
  pollHealth = null,
}: {
  onSelect?: () => void;
  pollHealth?: PollHealth | null;
}) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Header
        leagueName="Dynasty Warriors"
        subtitle="Week 3"
        meta="14-team full-PPR"
        screen="draft"
        onScreen={() => {}}
        polling
        pollHealth={pollHealth}
        onUndo={() => {}}
        chatOpen={false}
        onToggleChat={() => {}}
        settingsOpen={open}
        onToggleSettings={() => setOpen((o) => !o)}
        settingsRows={rows(onSelect)}
        footerNote="read-only connection"
      />
      <button type="button">After the header</button>
    </>
  );
}

// jsdom here has no storage of its own; give the preferences a scratch one.
const saved = new Map<string, string>();

beforeEach(() => {
  saved.clear();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => saved.get(key) ?? null,
    setItem: (key: string, value: string) => void saved.set(key, value),
    removeItem: (key: string) => void saved.delete(key),
  });
  resetPrefs();
});

afterEach(() => {
  vi.unstubAllGlobals();
  resetPrefs();
});

const openMenu = async (onSelect?: () => void) => {
  render(<Harness onSelect={onSelect} />);
  const gear = screen.getByRole("button", { name: "Settings" });
  expect(gear).toHaveAttribute("aria-haspopup", "menu");
  expect(gear).toHaveAttribute("aria-expanded", "false");
  await settle(() => {
    gear.click();
  });
  return gear;
};

describe("the settings menu", () => {
  it("is a menu, and each setting says whether it is on", async () => {
    await openMenu();
    expect(screen.getByRole("menu", { name: "Settings" })).toBeInTheDocument();

    const chime = screen.getByRole("menuitemcheckbox", { name: /Pick chime/ });
    expect(chime).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("menuitemcheckbox", { name: /Live sync/ })).toHaveAttribute(
      "aria-checked",
      "false",
    );
    expect(screen.getByRole("button", { name: "Settings" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
  });

  it("puts the keyboard on the first setting when it opens", async () => {
    await openMenu();
    expect(screen.getByRole("menuitemcheckbox", { name: /Pick chime/ })).toHaveFocus();
  });

  it("gives focus back to the gear when Escape closes it", async () => {
    const gear = await openMenu();
    const user = userEvent.setup();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(gear).toHaveFocus();
  });

  it("gives focus back to the gear when Done closes it", async () => {
    const gear = await openMenu();
    await settle(() => {
      screen.getByRole("menuitem", { name: "Done" }).click();
    });
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(gear).toHaveFocus();
  });

  it("closes itself when the keyboard leaves it, instead of hanging around", async () => {
    await openMenu();
    const user = userEvent.setup();
    // Tab out of the menu entirely: every item sits outside the tab order.
    await user.tab();
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    // And it does not snatch focus back to the gear the user just tabbed past.
    expect(screen.getByRole("button", { name: "Settings" })).not.toHaveFocus();
  });

  it("walks the settings with the arrow keys", async () => {
    await openMenu();
    const user = userEvent.setup();
    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("menuitemcheckbox", { name: /Live sync/ })).toHaveFocus();
    await user.keyboard("{End}");
    expect(screen.getByRole("menuitemcheckbox", { name: /Export state/ })).toHaveFocus();
    // Down from the last item comes back round to the top of the menu.
    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("menuitem", { name: "Done" })).toHaveFocus();
    await user.keyboard("{ArrowUp}");
    expect(screen.getByRole("menuitemcheckbox", { name: /Export state/ })).toHaveFocus();
  });

  it("still runs a setting when its row is chosen", async () => {
    const onSelect = vi.fn();
    await openMenu(onSelect);
    await settle(() => {
      screen.getByRole("menuitemcheckbox", { name: /Live sync/ }).click();
    });
    expect(onSelect).toHaveBeenCalledTimes(1);
  });
});

describe("the sync pill", () => {
  it("writes out why sync is failing rather than hiding it in a tooltip", () => {
    render(
      <Harness
        pollHealth={{
          last_success_at: null,
          consecutive_failures: 2,
          last_error: "network timeout",
        }}
      />,
    );
    expect(screen.getByText("Sync stale · 2 failures")).toBeInTheDocument();
    // Reachable by reading the page, with no mouse and no hovering.
    expect(screen.getByText("Last try failed: network timeout")).toBeInTheDocument();
  });

  it("says nothing extra while sync is healthy", () => {
    render(
      <Harness
        pollHealth={{ last_success_at: Date.now(), consecutive_failures: 0, last_error: null }}
      />,
    );
    expect(screen.getByText(/^Live · /)).toHaveClass("pill-live");
    expect(screen.queryByText(/Last try failed/)).not.toBeInTheDocument();
  });
});

describe("the chime button", () => {
  it("reads the preference itself, so nobody has to hand it down", async () => {
    render(<Harness />);
    const button = () => screen.getByRole("button", { name: "Pick chime on — click to mute" });
    expect(button()).toHaveAttribute("aria-pressed", "true");

    await settle(() => {
      button().click();
    });
    const muted = screen.getByRole("button", { name: "Pick chime muted" });
    expect(muted).toHaveAttribute("aria-pressed", "false");
    expect(saved.get("da.chime")).toBe("off");

    // And it follows the store when something else does the changing.
    await settle(() => {
      setChime(true);
    });
    expect(button()).toHaveAttribute("aria-pressed", "true");
  });
});
