import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ErrorBoundary } from "./ErrorBoundary";

function Bomb({ explode }: { explode: boolean }) {
  if (explode) throw new Error("survival_next is not a number");
  return <p>board rendered</p>;
}

describe("ErrorBoundary", () => {
  beforeEach(() => {
    // React reports caught render errors on console.error; keep the run quiet.
    vi.spyOn(console, "error").mockImplementation(() => undefined);
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("is invisible while nothing throws", () => {
    render(
      <ErrorBoundary>
        <Bomb explode={false} />
      </ErrorBoundary>,
    );
    expect(screen.getByText("board rendered")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("replaces a crashed tree with the error and a way back", async () => {
    const user = userEvent.setup();
    let explode = true;
    function Child() {
      return <Bomb explode={explode} />;
    }

    render(
      <ErrorBoundary>
        <Child />
      </ErrorBoundary>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("survival_next is not a number");
    expect(screen.queryByText("board rendered")).not.toBeInTheDocument();

    // The next live update fixed the data; reloading state must remount.
    explode = false;
    await user.click(screen.getByRole("button", { name: "Reload state" }));
    expect(screen.getByText("board rendered")).toBeInTheDocument();
  });
});
