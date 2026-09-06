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
import { HostWaiting, LaunchScreen } from "./SetupScreens";

afterEach(() => {
  vi.clearAllMocks();
});

function setup(activeLeagueId: string | null = null) {
  const onReady = vi.fn();
  render(
    <Setup
      onReady={onReady}
      onConnectYahoo={vi.fn()}
      onJoinHost={vi.fn()}
      activeLeagueId={activeLeagueId}
    />,
  );
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

describe("changing the username with a league already on screen", () => {
  // The Settings row led here, and the submit needed a league id typed in:
  // the user had come to change one word and was asked to find an id first.
  it("saves the username on its own and reloads the league it was given", async () => {
    const view = { league: { name: "Sunday Money" } };
    addLeague.mockResolvedValue(view);
    setMyUsername.mockResolvedValue("mcsleeper26");
    const { onReady, user } = setup("1389710366300200960");

    expect(screen.getByLabelText("League ID")).toHaveValue("1389710366300200960");
    expect(screen.getByRole("button", { name: "Save" })).toBeInTheDocument();
    await user.type(screen.getByLabelText("Sleeper username"), "mcsleeper26{Enter}");

    await waitFor(() => expect(onReady).toHaveBeenCalledWith(view));
    expect(setMyUsername).toHaveBeenCalledWith("mcsleeper26");
    expect(addLeague).toHaveBeenCalledWith("1389710366300200960");
  });

  it("goes back to loading once the id is changed to another league", async () => {
    const { user } = setup("1389710366300200960");
    await user.clear(screen.getByLabelText("League ID"));
    await user.type(screen.getByLabelText("League ID"), "42");
    expect(screen.getByRole("button", { name: "Load league" })).toBeInTheDocument();
  });
});

describe("the launch card on a follower", () => {
  function launch(overrides: Partial<Parameters<typeof LaunchScreen>[0]> = {}) {
    const onLeaveHost = vi.fn();
    render(
      <LaunchScreen
        leagueName="Dynasty Warriors"
        leagueId="1"
        platform="sleeper"
        attempt={2}
        maxAttempts={4}
        lastError="Justin's Mac did not answer within 10 seconds"
        onRetry={vi.fn()}
        onDifferentLeague={vi.fn()}
        hostName="Justin's Mac"
        onLeaveHost={onLeaveHost}
        {...overrides}
      />,
    );
    return onLeaveHost;
  }

  // A follower whose host accepted the connection and then went quiet sat on
  // this card with no controls; once the read times out it needs a way off
  // that does not pretend it can pick a league of its own.
  it("names the host it is waiting on and offers Leave host, not another league", async () => {
    const onLeaveHost = launch();
    expect(screen.getByText("Reconnecting to Justin's Mac, attempt 2 of 4")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Enter a different league" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Leave host" }));
    expect(onLeaveHost).toHaveBeenCalled();
  });

  it("still offers another league to a Mac running its own", () => {
    launch({ hostName: null, onLeaveHost: undefined });
    expect(screen.getByText("Reconnecting to Sleeper, attempt 2 of 4")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Enter a different league" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Leave host" })).toBeNull();
  });
});

describe("a follower whose host has nothing open", () => {
  it("can wait, or leave, instead of being handed the Sleeper form", async () => {
    const onRetry = vi.fn();
    const onLeaveHost = vi.fn();
    render(<HostWaiting hostName="Justin's Mac" onRetry={onRetry} onLeaveHost={onLeaveHost} />);

    expect(screen.getByText(/Justin's Mac has no league loaded/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    await userEvent.click(screen.getByRole("button", { name: "Leave host" }));
    expect(onRetry).toHaveBeenCalled();
    expect(onLeaveHost).toHaveBeenCalled();
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
