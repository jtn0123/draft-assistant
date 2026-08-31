import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ headshot: vi.fn() }));
vi.mock("../api", () => ({ api: mocks }));

import { resetAvatarCache, setAvatarMode } from "../avatars";
import { PlayerName, ZoomLayer } from "./bits";
import { closeZoom } from "../zoom";
import { settle } from "../test/settle";

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
    await settle(() => {
      button.click();
    });
    const dialog = screen.getByRole("dialog", { name: "Josh Downs" });
    expect(dialog.querySelector(".zoom-image")).toHaveAttribute(
      "src",
      "data:image/png;base64,AAAA",
    );

    await settle(() => {
      screen.getByRole("button", { name: "Close" }).click();
    });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});

describe("the zoom dialog and the keyboard", () => {
  const open = async () => {
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
    button.focus();
    await settle(() => {
      button.click();
    });
    return button;
  };

  it("moves focus into the dialog and back to the picture on close", async () => {
    const opener = await open();
    const close = screen.getByRole("button", { name: "Close" });
    expect(close).toHaveFocus();

    await settle(() => {
      close.click();
    });
    // Back where they were in the table, not at the top of the page.
    expect(opener).toHaveFocus();
  });

  it("keeps Tab inside the dialog instead of leaking to the page behind", async () => {
    await open();
    const close = screen.getByRole("button", { name: "Close" });
    const user = userEvent.setup();

    await user.tab();
    expect(close).toHaveFocus();
    await user.tab({ shift: true });
    expect(close).toHaveFocus();
  });

  it("closes on Escape and restores focus", async () => {
    const opener = await open();
    await settle(() => {
      fireEvent.keyDown(window, { key: "Escape" });
    });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(opener).toHaveFocus();
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
    await waitFor(() =>
      expect(screen.getAllByRole("presentation", { hidden: true })).toHaveLength(2),
    );
    expect(mocks.headshot).toHaveBeenCalledTimes(1);
  });

  it("spells out an injury code, so it is not left to a hover", () => {
    render(<PlayerName name="Josh Downs" team="IND" tag="Q" />);
    // The badge stays one letter wide on screen; the word is what is read out.
    expect(screen.getByText("Q")).toHaveAttribute("aria-hidden", "true");
    const tag = screen.getByText("Q").closest(".tag");
    expect(tag).toHaveTextContent("Questionable");
    expect(tag).toHaveAttribute("title", "Questionable");
  });

  it("leaves a tag that is already a word alone rather than saying it twice", () => {
    render(<PlayerName name="Josh Downs" team="IND" tag="IR" />);
    const tag = screen.getByText("IR");
    expect(tag).toHaveClass("tag");
    expect(tag.textContent).toBe("IR");
  });

  it("falls back to the team logo for defences and players without a photo", () => {
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
      expect(screen.getByRole("presentation", { hidden: true })).toHaveAttribute(
        "src",
        "data:image/png;base64,AAAA",
      ),
    );
    act(() => setAvatarMode("logos"));
    expect(screen.getByRole("presentation", { hidden: true })).toHaveAttribute(
      "src",
      "https://sleepercdn.com/images/team_logos/nfl/ind.png",
    );
    act(() => setAvatarMode("headshots"));
    await waitFor(() =>
      expect(screen.getByRole("presentation", { hidden: true })).toHaveAttribute(
        "src",
        "data:image/png;base64,AAAA",
      ),
    );
    expect(mocks.headshot).toHaveBeenCalledTimes(1);
  });

  it("draws a defence's logo from its id when no team is passed", () => {
    const { container } = render(
      <PlayerName name="Jacksonville Jaguars" team={null} playerId="JAX" />,
    );
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
