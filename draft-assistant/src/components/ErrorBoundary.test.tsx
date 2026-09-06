// The failure this guards is a screen chunk that never renders. The important
// assertion in every case is the same: something is on screen.

import { Suspense, lazy, useState } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ErrorBoundary } from "./ErrorBoundary";
import { firstFrames } from "./errorFrames";
import { reportError } from "../errorReport";

// The boundary's job here is to call the reporter, not to reach the backend;
// what the reporter itself does is `errorReport.test.ts`'s problem.
vi.mock("../errorReport", () => ({ reportError: vi.fn() }));

function Boom(): never {
  throw new Error("chunk missing");
}

// React itself logs every caught error; that noise is expected here, not a
// signal, so it is silenced and the assertions look at the DOM instead.
beforeEach(() => {
  vi.spyOn(console, "error").mockImplementation(() => undefined);
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("the error boundary", () => {
  it("shows a way out instead of unmounting the tree", () => {
    render(
      <div data-testid="shell">
        <ErrorBoundary>
          <Boom />
        </ErrorBoundary>
      </div>,
    );

    expect(screen.getByText(/could not be shown/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Reload" })).toBeInTheDocument();
    // The rest of the window survived: only the failing part was replaced.
    expect(screen.getByTestId("shell")).toBeInTheDocument();
  });

  it("is announced, since nothing else on the page says the screen went", () => {
    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(/could not be shown/);
  });

  it("catches a lazy screen whose chunk fails to arrive", async () => {
    const Missing = lazy(() =>
      Promise.reject(new Error("failed to fetch dynamically imported module")),
    );
    render(
      <ErrorBoundary>
        <Suspense fallback={<span>Loading…</span>}>
          <Missing />
        </Suspense>
      </ErrorBoundary>,
    );

    await waitFor(() => expect(screen.getByText(/could not be shown/)).toBeInTheDocument());
    expect(screen.queryByText("Loading…")).not.toBeInTheDocument();
  });

  it("reloads the window when asked", async () => {
    const reload = vi.fn();
    Object.defineProperty(window, "location", {
      configurable: true,
      value: { ...window.location, reload },
    });

    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );
    await userEvent.click(screen.getByRole("button", { name: "Reload" }));
    expect(reload).toHaveBeenCalled();
  });

  it("stays out of the way when nothing is wrong", () => {
    render(
      <ErrorBoundary>
        <span>the season screen</span>
      </ErrorBoundary>,
    );
    expect(screen.getByText("the season screen")).toBeInTheDocument();
    expect(screen.queryByText(/could not be shown/)).not.toBeInTheDocument();
  });
});

describe("one boundary per screen", () => {
  /** The shell's shape: two screens, each in a boundary, in the same slot. */
  function Shell() {
    const [screen, setScreen] = useState<"season" | "draft">("season");
    return (
      <>
        <button type="button" onClick={() => setScreen("draft")}>
          Draft
        </button>
        {screen === "season" ? (
          <ErrorBoundary key="season">
            <Boom />
          </ErrorBoundary>
        ) : (
          <ErrorBoundary key="draft">
            <span>the draft board</span>
          </ErrorBoundary>
        )}
      </>
    );
  }

  it("does not carry the season screen's crash over to the draft board", async () => {
    // Both branches put a boundary in the same place in the tree, so React
    // reused one instance: once the season screen had crashed, switching to
    // the draft showed the same "could not be shown" over a board that was
    // fine, and switching back and forth never cleared it.
    render(<Shell />);
    expect(screen.getByRole("alert")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Draft" }));

    expect(screen.getByText("the draft board")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});

describe("what gets reported", () => {
  it("reports the failure so it survives the dismissed screen", () => {
    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );
    expect(reportError).toHaveBeenCalledWith(
      expect.stringMatching(/^Error: chunk missing/),
      "render",
    );
  });

  it("names where in the tree it happened, not only what was thrown", () => {
    // "TypeError: x is undefined" on its own named no screen; the first two
    // component frames say which one, without the whole stack in the log.
    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );
    const message = String(vi.mocked(reportError).mock.calls[0]?.[0]);
    expect(message).toContain("Boom");
    expect(message.split(" < ")).toHaveLength(2);
  });

  it("keeps the first two frames of a component stack, trimmed", () => {
    expect(
      firstFrames("\n    at Boom (app.js:1)\n    at Suspense\n    at ErrorBoundary\n"),
    ).toEqual(["at Boom (app.js:1)", "at Suspense"]);
    expect(firstFrames(null)).toEqual([]);
  });
});

describe("Copy details", () => {
  it("hands over the details and says so", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { ...navigator, clipboard: { writeText } });

    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );
    await userEvent.click(screen.getByRole("button", { name: "Copy details" }));

    expect(writeText).toHaveBeenCalledTimes(1);
    // The message, not just "something went wrong": the whole point of the
    // button is that the person reading the paste learns something.
    expect(String(writeText.mock.calls[0]?.[0])).toContain("chunk missing");
    // The button used to fire and say nothing, so a paste that came up empty
    // had no explanation either way.
    expect(await screen.findByText("Copied")).toBeInTheDocument();
  });

  it("says why when the clipboard refuses", async () => {
    const writeText = vi.fn().mockRejectedValue(new Error("denied"));
    vi.stubGlobal("navigator", { ...navigator, clipboard: { writeText } });

    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );
    await userEvent.click(screen.getByRole("button", { name: "Copy details" }));
    expect(await screen.findByText("Could not copy: denied")).toBeInTheDocument();
  });

  it("says copying is unavailable rather than doing nothing", async () => {
    vi.stubGlobal("navigator", { ...navigator, clipboard: undefined });

    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );
    await userEvent.click(screen.getByRole("button", { name: "Copy details" }));
    expect(await screen.findByText("Copying is not available here")).toBeInTheDocument();
  });
});
