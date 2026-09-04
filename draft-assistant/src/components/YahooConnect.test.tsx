// The Connect Yahoo dialog: the three steps, in order, and what each of them
// does when the backend says no. Everything is asserted through what a user
// sees and types, and the backend is the mock the whole app's tests use.

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { settle } from "../test/settle";

const mocks = vi.hoisted(() => ({
  yahooStatus: vi.fn(),
  yahooSaveCredentials: vi.fn(),
  yahooBeginConnect: vi.fn(),
  yahooFinishConnect: vi.fn(),
  yahooDisconnect: vi.fn(),
  yahooLeagues: vi.fn(),
}));
vi.mock("../api", () => ({ api: mocks }));

import { YahooConnect } from "./YahooConnect";
import type { StoredLeague, YahooStatus } from "../types";

const status = (overrides: Partial<YahooStatus> = {}): YahooStatus => ({
  configured: false,
  connected: false,
  redirect: "oob",
  account: null,
  ...overrides,
});

const onSwitch = vi.fn();
const onStatus = vi.fn();
const onClose = vi.fn();

/** Render the dialog and let the opening status lookup land. */
async function open(first: YahooStatus = status()) {
  mocks.yahooStatus.mockResolvedValue(first);
  await settle(() => {
    render(
      <YahooConnect
        activeId="1"
        busy={false}
        onSwitch={onSwitch}
        onStatus={onStatus}
        onClose={onClose}
      />,
    );
  });
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("the credentials step", () => {
  it("says what to register with Yahoo, and which redirect the app expects", async () => {
    await open(status({ redirect: "oob" }));
    expect(screen.getByText(/developer.yahoo.com\/apps\/create/)).toBeInTheDocument();
    expect(screen.getByText("Installed Application")).toBeInTheDocument();
    expect(screen.getByText("Read")).toBeInTheDocument();
    expect(screen.getByText("oob")).toBeInTheDocument();
  });

  it("keeps the secret out of sight and will not save half a pair", async () => {
    const user = userEvent.setup();
    await open();
    const save = screen.getByRole("button", { name: "Save credentials" });
    expect(save).toBeDisabled();

    await user.type(screen.getByLabelText("Client id"), "dj0yJm");
    expect(save).toBeDisabled();

    const secret = screen.getByLabelText("Client secret");
    expect(secret).toHaveAttribute("type", "password");
    await user.type(secret, "abc123");
    expect(save).toBeEnabled();
  });

  it("saves the pair, forgets the secret, and moves on to signing in", async () => {
    const user = userEvent.setup();
    await open();
    mocks.yahooSaveCredentials.mockResolvedValue(status({ configured: true }));

    await user.type(screen.getByLabelText("Client id"), " dj0yJm ");
    await user.type(screen.getByLabelText("Client secret"), " abc123 ");
    await settle(() => screen.getByRole("button", { name: "Save credentials" }).click());

    expect(mocks.yahooSaveCredentials).toHaveBeenCalledWith("dj0yJm", "abc123");
    expect(onStatus).toHaveBeenCalledWith(status({ configured: true }));
    // The step changed, so the secret field is gone with the secret in it.
    expect(screen.queryByLabelText("Client secret")).toBeNull();
    expect(screen.getByRole("button", { name: "Sign in to Yahoo" })).toBeInTheDocument();
  });

  it("says why the credentials were refused and stays on the step", async () => {
    const user = userEvent.setup();
    await open();
    mocks.yahooSaveCredentials.mockRejectedValue(new Error("client id looks wrong"));

    await user.type(screen.getByLabelText("Client id"), "nope");
    await user.type(screen.getByLabelText("Client secret"), "nope");
    await settle(() => screen.getByRole("button", { name: "Save credentials" }).click());

    expect(screen.getByText("client id looks wrong")).toBeInTheDocument();
    expect(screen.getByLabelText("Client secret")).toBeInTheDocument();
  });
});

describe("the sign-in step", () => {
  const configured = status({ configured: true });

  it("shows the address to open by hand, and copies it", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { ...navigator, clipboard: { writeText } });
    await open(configured);
    mocks.yahooBeginConnect.mockResolvedValue({
      authorize_url: "https://api.login.yahoo.com/oauth2/request_auth?client_id=x",
      state: "s-1",
      redirect: "oob",
    });

    await settle(() => screen.getByRole("button", { name: "Sign in to Yahoo" }).click());
    expect(
      screen.getByText("https://api.login.yahoo.com/oauth2/request_auth?client_id=x"),
    ).toBeInTheDocument();

    await settle(() => screen.getByRole("button", { name: "Copy" }).click());
    expect(writeText).toHaveBeenCalledWith(
      "https://api.login.yahoo.com/oauth2/request_auth?client_id=x",
    );
    expect(screen.getByRole("button", { name: "Copied" })).toBeInTheDocument();
    vi.unstubAllGlobals();
  });

  it("finishes with the code Yahoo showed, handing back the state it started with", async () => {
    const user = userEvent.setup();
    await open(configured);
    mocks.yahooBeginConnect.mockResolvedValue({
      authorize_url: "https://yahoo.example/auth",
      state: "s-1",
      redirect: "oob",
    });
    mocks.yahooFinishConnect.mockResolvedValue(
      status({ configured: true, connected: true, account: "jtn0123" }),
    );

    await settle(() => screen.getByRole("button", { name: "Sign in to Yahoo" }).click());
    await user.type(screen.getByLabelText("Code from Yahoo"), " xy7q9 ");
    await settle(() => screen.getByRole("button", { name: "Finish" }).click());

    expect(mocks.yahooFinishConnect).toHaveBeenCalledWith("xy7q9", "s-1");
    expect(screen.getByText("jtn0123")).toBeInTheDocument();
  });

  it("finishes on Enter too, so a pasted code never needs the mouse", async () => {
    const user = userEvent.setup();
    await open(configured);
    mocks.yahooBeginConnect.mockResolvedValue({
      authorize_url: "https://yahoo.example/auth",
      state: "s-2",
      redirect: "oob",
    });
    mocks.yahooFinishConnect.mockResolvedValue(status({ configured: true, connected: true }));

    await settle(() => screen.getByRole("button", { name: "Sign in to Yahoo" }).click());
    await user.type(screen.getByLabelText("Code from Yahoo"), "xy7q9{Enter}");
    expect(mocks.yahooFinishConnect).toHaveBeenCalledWith("xy7q9", "s-2");
  });

  it("says why the browser trip could not be started", async () => {
    await open(configured);
    mocks.yahooBeginConnect.mockRejectedValue(new Error("no client id saved"));
    await settle(() => screen.getByRole("button", { name: "Sign in to Yahoo" }).click());

    expect(screen.getByText("no client id saved")).toBeInTheDocument();
    expect(screen.queryByLabelText("Code from Yahoo")).toBeNull();
  });

  it("retries a refused code against the same sign-in rather than starting over", async () => {
    const user = userEvent.setup();
    await open(configured);
    mocks.yahooBeginConnect.mockResolvedValue({
      authorize_url: "https://yahoo.example/auth",
      state: "s-4",
      redirect: "oob",
    });
    mocks.yahooFinishConnect.mockRejectedValueOnce(
      new Error("Yahoo rejected that code — check it and try again"),
    );

    await settle(() => screen.getByRole("button", { name: "Sign in to Yahoo" }).click());
    await user.type(screen.getByLabelText("Code from Yahoo"), "typo");
    await settle(() => screen.getByRole("button", { name: "Finish" }).click());
    expect(screen.getByText(/rejected that code/)).toBeInTheDocument();

    // The corrected code goes back with the state the sign-in started with —
    // one browser trip, not two.
    mocks.yahooFinishConnect.mockResolvedValue(status({ configured: true, connected: true }));
    await user.clear(screen.getByLabelText("Code from Yahoo"));
    await user.type(screen.getByLabelText("Code from Yahoo"), "xy7q9");
    await settle(() => screen.getByRole("button", { name: "Finish" }).click());

    expect(mocks.yahooFinishConnect).toHaveBeenCalledTimes(2);
    expect(mocks.yahooFinishConnect).toHaveBeenLastCalledWith("xy7q9", "s-4");
    expect(mocks.yahooBeginConnect).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Find my Yahoo leagues" })).toBeInTheDocument();
  });

  it("says why a code was refused and leaves it there to try again", async () => {
    const user = userEvent.setup();
    await open(configured);
    mocks.yahooBeginConnect.mockResolvedValue({
      authorize_url: "https://yahoo.example/auth",
      state: "s-3",
      redirect: "oob",
    });
    mocks.yahooFinishConnect.mockRejectedValue(new Error("that code has expired"));

    await settle(() => screen.getByRole("button", { name: "Sign in to Yahoo" }).click());
    await user.type(screen.getByLabelText("Code from Yahoo"), "stale");
    await settle(() => screen.getByRole("button", { name: "Finish" }).click());

    expect(screen.getByText("that code has expired")).toBeInTheDocument();
    expect(screen.getByLabelText("Code from Yahoo")).toHaveValue("stale");
  });
});

describe("the connected step", () => {
  const connected = status({ configured: true, connected: true, account: "jtn0123" });
  const office: StoredLeague = {
    league_id: "449.l.98765",
    name: "Office League",
    season: "2026",
    status: "pre_draft",
    platform: "yahoo",
  };

  it("names the account and says the connection only reads", async () => {
    await open(connected);
    expect(screen.getByText("jtn0123")).toBeInTheDocument();
    expect(screen.getByText(/read-only connection/)).toBeInTheDocument();
  });

  it("lists the account's leagues, marked as Yahoo's, and switches to one", async () => {
    await open(connected);
    mocks.yahooLeagues.mockResolvedValue([office]);
    await settle(() => screen.getByRole("button", { name: "Find my Yahoo leagues" }).click());

    const row = screen.getByRole("button", { name: /Office League/ });
    expect(row).toHaveTextContent("Yahoo");
    expect(row).toHaveTextContent("draft ahead");
    await settle(() => row.click());
    expect(onSwitch).toHaveBeenCalledWith("449.l.98765");
    expect(screen.getByRole("button", { name: "Look again" })).toBeInTheDocument();
  });

  it("says plainly when the account plays in nothing", async () => {
    await open(connected);
    mocks.yahooLeagues.mockResolvedValue([]);
    await settle(() => screen.getByRole("button", { name: "Find my Yahoo leagues" }).click());
    expect(screen.getByText(/plays in no fantasy football leagues/)).toBeInTheDocument();
  });

  it("says why the lookup failed", async () => {
    await open(connected);
    mocks.yahooLeagues.mockRejectedValue(new Error("Yahoo is throttling us"));
    await settle(() => screen.getByRole("button", { name: "Find my Yahoo leagues" }).click());
    expect(screen.getByText("Yahoo is throttling us")).toBeInTheDocument();
  });

  it("says that disconnecting keeps the registered app", async () => {
    await open(connected);
    expect(screen.getByText(/keeps the app you registered/)).toBeInTheDocument();
  });

  it("disconnects back to signing in, keeping the saved app", async () => {
    await open(connected);
    mocks.yahooLeagues.mockResolvedValue([office]);
    await settle(() => screen.getByRole("button", { name: "Find my Yahoo leagues" }).click());
    mocks.yahooDisconnect.mockResolvedValue(status({ configured: true }));
    await settle(() => screen.getByRole("button", { name: "Disconnect" }).click());

    expect(onStatus).toHaveBeenLastCalledWith(status({ configured: true }));
    expect(screen.getByRole("button", { name: "Sign in to Yahoo" })).toBeInTheDocument();
    // The list belonged to the account that just went away.
    expect(screen.queryByRole("button", { name: /Office League/ })).toBeNull();
  });

  it("says why a disconnect failed and stays connected", async () => {
    await open(connected);
    mocks.yahooDisconnect.mockRejectedValue(new Error("keychain is locked"));
    await settle(() => screen.getByRole("button", { name: "Disconnect" }).click());

    expect(screen.getByText("keychain is locked")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Find my Yahoo leagues" })).toBeInTheDocument();
  });
});

describe("the dialog itself", () => {
  it("says why it could not even read the status", async () => {
    mocks.yahooStatus.mockRejectedValue(new Error("Yahoo needs the desktop app"));
    await settle(() => {
      render(
        <YahooConnect
          activeId={null}
          busy={false}
          onSwitch={onSwitch}
          onStatus={onStatus}
          onClose={onClose}
        />,
      );
    });
    expect(screen.getByText("Yahoo needs the desktop app")).toBeInTheDocument();
    expect(onStatus).not.toHaveBeenCalled();
  });

  it("closes from its button, from the scrim, and from Escape", async () => {
    const user = userEvent.setup();
    await open();
    await settle(() => screen.getByRole("button", { name: "Close" }).click());
    expect(onClose).toHaveBeenCalledTimes(1);

    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(2);

    await settle(() => screen.getByRole("dialog").parentElement?.click());
    expect(onClose).toHaveBeenCalledTimes(3);
  });
});
