// The failure this guards: a user who cannot say what went wrong, and a
// diagnostics block that says too much. Both halves are asserted here — that
// the dialog shows the facts, and that what it copies carries no secret.

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Diagnostics } from "./Diagnostics";
import { diagnosticsText, pollSummary } from "./diagnosticsText";
import { diagnostics, harness } from "../test/appHarness";

vi.mock("../api", async () => ({ api: (await import("../test/appHarness")).harness().api }));

const { api, reset } = harness();

beforeEach(() => {
  reset();
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
