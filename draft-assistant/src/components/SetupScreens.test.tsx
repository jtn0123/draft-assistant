// The first screen anyone sees: what it does with a keyboard, and what it says
// when the load fails.

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

const setMyUsername = vi.fn<(username: string) => Promise<string>>();
const addLeague = vi.fn<(leagueId: string) => Promise<unknown>>();

vi.mock("../api", () => ({
  api: {
    setMyUsername: (username: string) => setMyUsername(username),
    addLeague: (leagueId: string) => addLeague(leagueId),
  },
}));

import { Setup } from "./Panels";

afterEach(() => {
  vi.clearAllMocks();
});

function setup() {
  const onReady = vi.fn();
  render(<Setup onReady={onReady} onConnectYahoo={vi.fn()} onJoinHost={vi.fn()} />);
  return { onReady, user: userEvent.setup() };
}

describe("the first-launch form", () => {
  // Two text inputs and a button only the mouse could reach. Typing an id and
  // pressing Return did nothing at all, which reads as a broken app on the
  // very first screen.
  it("loads the league when Return is pressed in the league id field", async () => {
    addLeague.mockResolvedValue({ league: { name: "Sunday Money" } });
    const { onReady, user } = setup();

    await user.type(screen.getByLabelText("League ID"), "1389710366300200960{Enter}");

    await waitFor(() => expect(addLeague).toHaveBeenCalledWith("1389710366300200960"));
    expect(onReady).toHaveBeenCalled();
  });

  it("loads the league when Return is pressed in the username field", async () => {
    addLeague.mockResolvedValue({ league: { name: "Sunday Money" } });
    setMyUsername.mockResolvedValue("mcsleeper26");
    const { user } = setup();

    await user.type(screen.getByLabelText("League ID"), "123");
    await user.type(screen.getByLabelText("Sleeper username"), "mcsleeper26{Enter}");

    await waitFor(() => expect(setMyUsername).toHaveBeenCalledWith("mcsleeper26"));
    expect(addLeague).toHaveBeenCalledWith("123");
  });

  it("does nothing on Return while there is no league id to load", async () => {
    const { user } = setup();
    await user.type(screen.getByLabelText("Sleeper username"), "mcsleeper26{Enter}");
    expect(addLeague).not.toHaveBeenCalled();
    expect(setMyUsername).not.toHaveBeenCalled();
  });
});

describe("what the first screen says when the load fails", () => {
  // `String(e)` on an Error built from another error printed "Error: Error:"
  // — twice over, on the one screen with nothing else on it.
  it("shows the message without the Error prefix", async () => {
    addLeague.mockRejectedValue(new Error("league 123 was not found on Sleeper"));
    const { user } = setup();

    await user.type(screen.getByLabelText("League ID"), "123{Enter}");

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("league 123 was not found on Sleeper");
    expect(alert.textContent).not.toContain("Error:");
  });

  // Announced, because the button goes back to saying "Load league" and the
  // only other thing that changed is a line of red text further down.
  it("announces the failure rather than leaving it to be noticed", async () => {
    addLeague.mockRejectedValue("the host is away");
    const { user } = setup();

    await user.type(screen.getByLabelText("League ID"), "123{Enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("the host is away");
  });
});
