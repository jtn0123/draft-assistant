import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SeasonView } from "../season-types";

const mocks = vi.hoisted(() => ({ refreshSeason: vi.fn() }));
vi.mock("../api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../api")>();
  return { ...actual, api: { ...actual.api, refreshSeason: mocks.refreshSeason } };
});

import { view } from "./season-screen-fixture";
import { SeasonScreen } from "./SeasonScreen";

/** The fixture view with a different projection, so a refresh is visible. */
function refreshedView(): SeasonView {
  const base = view();
  return { ...base, header: { ...base.header, my_projected: 130.2 } };
}

beforeEach(() => {
  mocks.refreshSeason.mockReset();
});

// The backend has had `refresh_season` (with its own week-rollover check) for
// a while; the desktop never called it. These pin the button that does.
describe("the Refresh button", () => {
  it("asks the backend for the season again and shows what came back", async () => {
    mocks.refreshSeason.mockResolvedValue(refreshedView());
    render(<SeasonScreen view={view()} />);
    expect(screen.getByText("vs punt_god · 122.4 – 108.9")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));

    expect(await screen.findByText("vs punt_god · 130.2 – 108.9")).toBeInTheDocument();
    expect(mocks.refreshSeason).toHaveBeenCalledTimes(1);
  });

  it("says it is working, and refuses a second click, until the answer lands", async () => {
    let finish: (v: SeasonView) => void = () => undefined;
    mocks.refreshSeason.mockReturnValue(
      new Promise<SeasonView>((resolve) => {
        finish = resolve;
      }),
    );
    render(<SeasonScreen view={view()} />);

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    const busy = await screen.findByRole("button", { name: "Refreshing…" });
    expect(busy).toBeDisabled();
    fireEvent.click(busy);
    expect(mocks.refreshSeason).toHaveBeenCalledTimes(1);

    await act(async () => {
      finish(refreshedView());
      await Promise.resolve();
    });
    expect(screen.getByRole("button", { name: "Refresh" })).toBeEnabled();
  });

  it("says why when it fails, keeps the view it had, and offers the button again", async () => {
    mocks.refreshSeason.mockRejectedValue("no league loaded");
    render(<SeasonScreen view={view()} />);

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Could not refresh the season: no league loaded");
    expect(screen.getByText("vs punt_god · 122.4 – 108.9")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Refresh" })).toBeEnabled();
  });

  it("lets a later push from the poller replace what Refresh brought back", async () => {
    mocks.refreshSeason.mockResolvedValue(refreshedView());
    const { rerender } = render(<SeasonScreen view={view()} />);
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(await screen.findByText("vs punt_god · 130.2 – 108.9")).toBeInTheDocument();

    const base = view();
    rerender(<SeasonScreen view={{ ...base, header: { ...base.header, my_projected: 99.9 } }} />);
    expect(screen.getByText("vs punt_god · 99.9 – 108.9")).toBeInTheDocument();
    expect(screen.queryByText("vs punt_god · 130.2 – 108.9")).not.toBeInTheDocument();
  });
});
