// The switch-league dialog: what it offers, and what each way of choosing
// does. Everything here is asserted through what a user sees and clicks.

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { settle } from "../test/settle";

const mocks = vi.hoisted(() => ({ sleeperLeagues: vi.fn() }));
vi.mock("../api", () => ({ api: mocks }));

import { LeaguePicker } from "./LeaguePicker";
import type { StoredLeague } from "../types";

const known: StoredLeague[] = [
  { league_id: "1", name: "Dynasty Warriors", season: "2026" },
  { league_id: "2", name: "Mock draft", season: "2026" },
];

const onSwitch = vi.fn();
const onClose = vi.fn();

function open(props: Partial<Parameters<typeof LeaguePicker>[0]> = {}) {
  render(
    <LeaguePicker
      leagues={known}
      activeId="1"
      season="2026"
      busy={false}
      onSwitch={onSwitch}
      onClose={onClose}
      {...props}
    />,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.sleeperLeagues.mockResolvedValue([]);
});

describe("the leagues it offers", () => {
  it("lists what the app has loaded, with the one on screen marked", () => {
    open();
    const active = screen.getByRole("button", { name: /Dynasty Warriors/ });
    expect(active).toHaveAttribute("aria-pressed", "true");
    expect(active).toHaveTextContent("on screen now");
    expect(screen.getByRole("button", { name: /Mock draft/ })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });

  it("switches to another one, and just closes on the one already showing", async () => {
    open();
    await settle(() => screen.getByRole("button", { name: /Mock draft/ }).click());
    expect(onSwitch).toHaveBeenCalledWith("2");

    await settle(() => screen.getByRole("button", { name: /Dynasty Warriors/ }).click());
    expect(onSwitch).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalled();
  });
});

describe("looking the account up on Sleeper", () => {
  it("asks for the season on screen and adds what comes back", async () => {
    mocks.sleeperLeagues.mockResolvedValue([
      { league_id: "3", name: "Work league", season: "2026" },
    ]);
    open();
    await settle(() => screen.getByRole("button", { name: /Find my leagues/ }).click());

    expect(mocks.sleeperLeagues).toHaveBeenCalledWith("2026");
    expect(screen.getByRole("button", { name: /Work league/ })).toBeInTheDocument();
  });

  it("says why the lookup failed and leaves the known leagues alone", async () => {
    mocks.sleeperLeagues.mockRejectedValue(new Error("no Sleeper account saved"));
    open();
    await settle(() => screen.getByRole("button", { name: /Find my leagues/ }).click());

    expect(screen.getByText("no Sleeper account saved")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Mock draft/ })).toBeInTheDocument();
  });
});

describe("pasting a league or draft", () => {
  it("will not load an empty box, and loads a pasted id", async () => {
    const user = userEvent.setup();
    open();
    expect(screen.getByRole("button", { name: "Load" })).toBeDisabled();

    await user.type(screen.getByLabelText(/Or paste a league or draft/), " 1389710366 ");
    await user.click(screen.getByRole("button", { name: "Load" }));

    expect(onSwitch).toHaveBeenCalledWith("1389710366");
  });

  it("loads on Enter, so a pasted id never needs the mouse", async () => {
    const user = userEvent.setup();
    open();
    await user.type(screen.getByLabelText(/Or paste a league or draft/), "1389710366{Enter}");
    expect(onSwitch).toHaveBeenCalledWith("1389710366");
  });
});

describe("while a switch is running", () => {
  it("stops every way of starting a second one", () => {
    open({ busy: true });
    expect(screen.getByRole("button", { name: /Mock draft/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Find my leagues/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Loading…" })).toBeDisabled();
  });
});
