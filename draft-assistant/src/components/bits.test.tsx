import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ headshot: vi.fn() }));
vi.mock("../api", () => ({ api: mocks }));

import { resetAvatarCache, setAvatarMode } from "../avatars";
import { PlayerName, ZoomLayer } from "./bits";
import { closeZoom } from "../zoom";

beforeEach(() => {
  vi.clearAllMocks();
  resetAvatarCache();
  setAvatarMode("headshots");
  closeZoom();
});

describe("clicking a picture", () => {
  it("opens the larger copy, captioned, and closes again", async () => {
    mocks.headshot.mockResolvedValue("data:image/png;base64,AAAA");
    render(
      <>
        <PlayerName name="Josh Downs" team="IND" playerId="11560" />
        <ZoomLayer />
      </>,
    );
    const button = await screen.findByRole("button", {
      name: "Show a larger picture of Josh Downs",
    });
    await act(async () => {
      button.click();
    });
    const dialog = screen.getByRole("dialog", { name: "Josh Downs" });
    expect(dialog.querySelector(".zoom-image")).toHaveAttribute("src", "data:image/png;base64,AAAA");

    await act(async () => {
      screen.getByRole("button", { name: "Close" }).click();
    });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});

describe("PlayerName", () => {
  it("shows the cached headshot once the backend resolves it", async () => {
    mocks.headshot.mockResolvedValue("data:image/png;base64,AAAA");
    render(<PlayerName name="Josh Downs" team="IND" playerId="11560" />);
    await waitFor(() =>
      expect(screen.getByRole("presentation", { hidden: true })).toHaveAttribute(
        "src",
        "data:image/png;base64,AAAA",
      ),
    );
    expect(mocks.headshot).toHaveBeenCalledWith("11560");
  });

  it("asks the backend once per player even when drawn many times", async () => {
    mocks.headshot.mockResolvedValue("data:image/png;base64,AAAA");
    render(
      <>
        <PlayerName name="Josh Downs" team="IND" playerId="11560" />
        <PlayerName name="Josh Downs" team="IND" playerId="11560" />
      </>,
    );
    await waitFor(() => expect(screen.getAllByRole("presentation", { hidden: true })).toHaveLength(2));
    expect(mocks.headshot).toHaveBeenCalledTimes(1);
  });

  it("falls back to the team logo for defences and players without a photo", async () => {
    mocks.headshot.mockResolvedValue(null);
    render(<PlayerName name="Detroit Lions" team="DET" playerId="DET" />);
    expect(screen.getByRole("presentation", { hidden: true })).toHaveAttribute(
      "src",
      "https://sleepercdn.com/images/team_logos/nfl/det.png",
    );
    expect(mocks.headshot).not.toHaveBeenCalled();
  });

  it("switching the setting to team logos drops the photos without a refetch", async () => {
    mocks.headshot.mockResolvedValue("data:image/png;base64,AAAA");
    render(<PlayerName name="Josh Downs" team="IND" playerId="11560" />);
    await waitFor(() =>
      expect(screen.getByRole("presentation", { hidden: true })).toHaveAttribute("src", "data:image/png;base64,AAAA"),
    );
    act(() => setAvatarMode("logos"));
    expect(screen.getByRole("presentation", { hidden: true })).toHaveAttribute(
      "src",
      "https://sleepercdn.com/images/team_logos/nfl/ind.png",
    );
    act(() => setAvatarMode("headshots"));
    await waitFor(() =>
      expect(screen.getByRole("presentation", { hidden: true })).toHaveAttribute("src", "data:image/png;base64,AAAA"),
    );
    expect(mocks.headshot).toHaveBeenCalledTimes(1);
  });

  it("draws a defence's logo from its id when no team is passed", () => {
    const { container } = render(<PlayerName name="Jacksonville Jaguars" team={null} playerId="JAX" />);
    expect(container.querySelector(".avatar-logo")).toHaveAttribute(
      "src",
      "https://sleepercdn.com/images/team_logos/nfl/jax.png",
    );
    expect(mocks.headshot).not.toHaveBeenCalled();
  });

  it("gives the photo, the team mark and the blank the same slot", async () => {
    mocks.headshot.mockResolvedValue("data:image/png;base64,AAAA");
    const { container } = render(<PlayerName name="Josh Downs" team="IND" playerId="11560" />);
    await waitFor(() => expect(container.querySelector(".avatar.headshot")).not.toBeNull());
    act(() => setAvatarMode("logos"));
    expect(container.querySelector(".avatar.avatar-logo")).not.toBeNull();

    // A free agent has neither, and still occupies the row the same way.
    const free = render(<PlayerName name="Nobody" team={null} playerId="99999" />);
    expect(free.container.querySelector(".avatar.avatar-blank")).not.toBeNull();
  });
});
