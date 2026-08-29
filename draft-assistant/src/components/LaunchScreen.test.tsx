import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { startingLaunch } from "../launch";
import { LaunchScreen } from "./LaunchScreen";

describe("LaunchScreen", () => {
  it("says it is reading settings before there is a league to load", () => {
    render(<LaunchScreen status={null} onRetry={() => {}} onSetup={() => {}} />);
    expect(screen.getByRole("status")).toHaveTextContent("Reading your settings…");
  });

  it("says it is connecting on the first try, and still trying after", () => {
    const { rerender } = render(
      <LaunchScreen status={startingLaunch("1")} onRetry={() => {}} onSetup={() => {}} />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("Connecting to Sleeper");
    rerender(
      <LaunchScreen
        status={{ ...startingLaunch("1"), attempt: 3, error: "operation timed out" }}
        onRetry={() => {}}
        onSetup={() => {}}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("trying again (attempt 3 of 3)");
    expect(screen.getByText("operation timed out")).toBeInTheDocument();
  });

  it("gives up visibly with a retry and a way to another league", async () => {
    const user = userEvent.setup();
    const onRetry = vi.fn();
    const onSetup = vi.fn();
    render(
      <LaunchScreen
        status={{ ...startingLaunch("1"), attempt: 3, failed: true, error: "operation timed out" }}
        onRetry={onRetry}
        onSetup={onSetup}
      />,
    );
    expect(screen.getByRole("heading", { name: "Unable to connect" })).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("operation timed out");
    await user.click(screen.getByRole("button", { name: "Try again" }));
    await user.click(screen.getByRole("button", { name: "Load a different league" }));
    expect(onRetry).toHaveBeenCalledTimes(1);
    expect(onSetup).toHaveBeenCalledTimes(1);
  });
});
