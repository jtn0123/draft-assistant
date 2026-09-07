// The failure this guards: a user who cannot say what went wrong, and a
// diagnostics block that says too much. Both halves are asserted here — that
// the dialog shows the facts, and that what it copies carries no secret.

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Diagnostics } from "./Diagnostics";
import { diagnosticsText, pollSummary } from "./diagnosticsText";
import { diagnostics, harness } from "../test/appHarness";

vi.mock("../api", async () => ({ api: (await import("../test/appHarness")).harness().api }));

const { api, reset } = harness();

beforeEach(() => {
  reset();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

/** A clipboard this jsdom does not otherwise have. */
function stubClipboard() {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { ...navigator, clipboard: { writeText } });
  return writeText;
}

describe("the diagnostics dialog", () => {
  it("shows the league, the poller and where the log is", async () => {
    render(<Diagnostics appVersion="0.2.0" onClose={() => undefined} />);

    await waitFor(() => expect(screen.getByText("Dynasty Warriors · 1")).toBeInTheDocument());
    expect(screen.getByText("macos aarch64")).toBeInTheDocument();
    expect(screen.getByText("On, healthy")).toBeInTheDocument();
    expect(screen.getByText(/draft-assistant\.log$/)).toBeInTheDocument();
    expect(screen.getByText(/INFO polling started/)).toBeInTheDocument();
  });

  it("copies a block that names the league and carries no secret", async () => {
    const writeText = stubClipboard();
    api.diagnostics.mockResolvedValue(
      diagnostics({
        log_tail: ["2026-09-03T16:22:01Z ERROR yahoo refused: client_secret=····"],
      }),
    );
    render(<Diagnostics appVersion="0.2.0" onClose={() => undefined} />);

    await waitFor(() => expect(screen.getByText("Copy diagnostics")).toBeInTheDocument());
    await userEvent.click(screen.getByRole("button", { name: "Copy diagnostics" }));

    expect(writeText).toHaveBeenCalledTimes(1);
    const copied = String(writeText.mock.calls[0]?.[0]);
    expect(copied).toContain("Dynasty Warriors");
    expect(copied).toContain("Draft Assistant 0.2.0");
    expect(copied).toContain("--- log ---");
    // The pairing code and the key never went into the report in the first
    // place, and the log tail arrives already masked.
    expect(copied).not.toMatch(/\bcode\b\s*[:=]\s*\d{6}/);
    expect(copied).not.toContain("sk-ant");
    await waitFor(() => expect(screen.getByText("Copied")).toBeInTheDocument());
  });

  it("asks the backend to open the log folder", async () => {
    render(<Diagnostics appVersion="0.2.0" onClose={() => undefined} />);
    await waitFor(() => expect(screen.getByText("Open log folder")).toBeInTheDocument());
    await userEvent.click(screen.getByRole("button", { name: "Open log folder" }));
    expect(api.openLogFolder).toHaveBeenCalled();
  });

  it("hides the log actions and falls back to the shell's version on a follower", async () => {
    // What `apiRemote` reports: no log of its own, and no version to give.
    api.diagnostics.mockResolvedValue(
      diagnostics({
        app_version: "",
        platform: "following Justin's Mac",
        log_path: null,
        log_tail: [],
      }),
    );
    render(<Diagnostics appVersion="0.2.0" onClose={() => undefined} />);

    await waitFor(() => expect(screen.getByText("following Justin's Mac")).toBeInTheDocument());
    expect(screen.queryByRole("button", { name: "Open log folder" })).not.toBeInTheDocument();
    expect(screen.getByText("Nothing in the log yet.")).toBeInTheDocument();
    expect(screen.getByText("0.2.0")).toBeInTheDocument();
  });

  it("says so rather than showing nothing when the backend will not answer", async () => {
    api.diagnostics.mockRejectedValue(new Error("no league loaded"));
    render(<Diagnostics appVersion="0.2.0" onClose={() => undefined} />);
    await waitFor(() => expect(screen.getByText("no league loaded")).toBeInTheDocument());
  });

  it("still has a Close button when the backend will not answer", async () => {
    // Escape and the scrim worked, but a dialog whose only content is an
    // error and no button reads as stuck.
    const onClose = vi.fn();
    api.diagnostics.mockRejectedValue(new Error("no league loaded"));
    render(<Diagnostics appVersion="0.2.0" onClose={onClose} />);
    await userEvent.click(await screen.findByRole("button", { name: "Close" }));
    expect(onClose).toHaveBeenCalled();
  });

  it("says copying is unavailable rather than doing nothing", async () => {
    vi.stubGlobal("navigator", { ...navigator, clipboard: undefined });
    render(<Diagnostics appVersion="0.2.0" onClose={() => undefined} />);
    await userEvent.click(await screen.findByRole("button", { name: "Copy diagnostics" }));
    expect(await screen.findByText("Copying is not available here")).toBeInTheDocument();
  });

  it("forgets the last outcome when the next action starts", async () => {
    // "Copied" stayed on screen under the failure of whatever came next, so
    // the dialog reported two outcomes at once.
    stubClipboard();
    api.openLogFolder.mockRejectedValue(new Error("no such folder"));
    render(<Diagnostics appVersion="0.2.0" onClose={() => undefined} />);
    await userEvent.click(await screen.findByRole("button", { name: "Copy diagnostics" }));
    expect(await screen.findByText("Copied")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Open log folder" }));
    expect(await screen.findByText("no such folder")).toBeInTheDocument();
    expect(screen.queryByText("Copied")).toBeNull();
  });

  // "Copied" and "no such folder" land under the buttons, where a screen
  // reader on the button it just pressed hears nothing unless they are live.
  it("announces the outcome of a button politely, and a failure assertively", async () => {
    stubClipboard();
    api.openLogFolder.mockRejectedValue(new Error("no such folder"));
    render(<Diagnostics appVersion="0.2.0" onClose={() => undefined} />);
    await userEvent.click(await screen.findByRole("button", { name: "Copy diagnostics" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Copied");
    expect(screen.queryByRole("alert")).toBeNull();

    await userEvent.click(screen.getByRole("button", { name: "Open log folder" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("no such folder");
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("announces a backend that will not answer, in place of the reading line", async () => {
    api.diagnostics.mockRejectedValue(new Error("no league loaded"));
    render(<Diagnostics appVersion="0.2.0" onClose={() => undefined} />);
    expect(screen.getByRole("status")).toHaveTextContent("Reading…");
    expect(await screen.findByRole("alert")).toHaveTextContent("no league loaded");
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("closes on Escape", async () => {
    const onClose = vi.fn();
    render(<Diagnostics appVersion="0.2.0" onClose={onClose} />);
    await waitFor(() => expect(screen.getByText("Diagnostics")).toBeInTheDocument());
    await userEvent.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalled();
  });
});

describe("the poll summary", () => {
  it("names the failure rather than just saying something is wrong", () => {
    const failing = diagnostics({
      poll: { last_success_at: 1, consecutive_failures: 4, last_error: "sleeper timed out" },
    });
    expect(pollSummary(failing)).toContain("sleeper timed out");
    expect(pollSummary(failing)).toContain("4");
  });

  it("is Off when nothing is polling", () => {
    expect(pollSummary(diagnostics({ polling: false }))).toBe("Off");
  });
});

describe("the copied text", () => {
  it("leaves the log section out when there is no log", () => {
    const text = diagnosticsText(diagnostics({ log_tail: [], log_path: null }), "0.2.0");
    expect(text).not.toContain("--- log ---");
    expect(text).toContain("Log: none on this machine");
  });
});

describe("verbose logging", () => {
  // The failure this prevents: debug lines could only be turned on by
  // exporting an environment variable before launch, which nobody who
  // double-clicks the app can do.
  it("turns the level up and down from the dialog", async () => {
    render(<Diagnostics appVersion="0.2.0" onClose={() => undefined} />);
    const box = await screen.findByRole("checkbox", { name: /Verbose logging/ });
    expect(box).not.toBeChecked();

    await userEvent.click(box);
    expect(api.setLogLevel).toHaveBeenCalledWith("debug");
    await waitFor(() => expect(screen.getByText("Verbose logging is on")).toBeInTheDocument());
    expect(box).toBeChecked();

    await userEvent.click(box);
    expect(api.setLogLevel).toHaveBeenCalledWith("info");
    await waitFor(() => expect(screen.getByText("Verbose logging is off")).toBeInTheDocument());
  });

  it("shows the level the backend is actually using when it opens", async () => {
    api.diagnostics.mockResolvedValue(diagnostics({ log_level: "debug" }));
    render(<Diagnostics appVersion="0.2.0" onClose={() => undefined} />);
    expect(await screen.findByRole("checkbox", { name: /Verbose logging/ })).toBeChecked();
  });

  it("puts the checkbox back when the backend refuses", async () => {
    api.setLogLevel.mockRejectedValue(new Error("this app has no log file yet"));
    render(<Diagnostics appVersion="0.2.0" onClose={() => undefined} />);
    const box = await screen.findByRole("checkbox", { name: /Verbose logging/ });

    await userEvent.click(box);
    await waitFor(() =>
      expect(screen.getByText("this app has no log file yet")).toBeInTheDocument(),
    );
    expect(box).not.toBeChecked();
  });

  // A follower in a browser tab has no log file and no level to set; its
  // diagnostics come back with `log_path: null` (see apiRemoteLog.ts).
  it("is not offered on a browser follower, which has no log file of its own", async () => {
    api.diagnostics.mockResolvedValue(
      diagnostics({ platform: "following Justin's Mac", log_path: null, log_tail: [] }),
    );
    render(<Diagnostics appVersion="0.2.0" onClose={() => undefined} />);
    await waitFor(() => expect(screen.getByText("Nothing in the log yet.")).toBeInTheDocument());
    expect(screen.queryByRole("checkbox", { name: /Verbose logging/ })).not.toBeInTheDocument();
  });

  // A follower inside Tauri is still the desktop app, with a backend and a
  // log file of its own under it: apiRemoteLog.ts reports that file's path
  // and routes setLogLevel to that backend, so the checkbox is offered.
  it("is offered on a Tauri follower, whose own backend keeps a log", async () => {
    api.diagnostics.mockResolvedValue(
      diagnostics({
        platform: "following Justin's Mac",
        log_path: "/Users/me/Library/Logs/draft-assistant/draft-assistant.log",
      }),
    );
    render(<Diagnostics appVersion="0.2.0" onClose={() => undefined} />);
    const box = await screen.findByRole("checkbox", { name: /Verbose logging/ });
    await userEvent.click(box);
    expect(api.setLogLevel).toHaveBeenCalledWith("debug");
  });
});
