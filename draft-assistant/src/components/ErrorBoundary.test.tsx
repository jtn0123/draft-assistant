// The failure this guards is a screen chunk that never renders. The important
// assertion in every case is the same: something is on screen.

import { Suspense, lazy } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ErrorBoundary } from "./ErrorBoundary";

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
