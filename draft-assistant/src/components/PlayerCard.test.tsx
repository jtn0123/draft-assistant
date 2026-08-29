import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import fixtureJson from "../../public/dev-fixture.json";
import type { DraftView } from "../types";
import { PlayerCardProvider, PlayerName } from "./PlayerCard";

function view(): DraftView {
  return structuredClone(fixtureJson) as unknown as DraftView;
}

describe("PlayerCard", () => {
  it("is plain text without a provider", () => {
    render(<PlayerName id="x">Some Name</PlayerName>);
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.getByText("Some Name")).toBeInTheDocument();
  });

  it("opens a card with the facts on a tap and closes on Escape", async () => {
    const user = userEvent.setup();
    const v = view();
    const me = v.rosters.find((r) => r.slot === v.draft.my_slot)!;
    const p = me.players[0];
    render(
      <PlayerCardProvider view={v}>
        <PlayerName id={p.player_id}>{p.name}</PlayerName>
      </PlayerCardProvider>,
    );
    await user.click(screen.getByRole("button", { name: p.name }));
    const card = screen.getByRole("dialog", { name: p.name });
    expect(card).toHaveTextContent("Owner");
    expect(card).toHaveTextContent("YOU");
    expect(card).toHaveTextContent(`round ${p.round}`);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("says nothing for an id the view does not know", async () => {
    const user = userEvent.setup();
    render(
      <PlayerCardProvider view={view()}>
        <PlayerName id="ghost">Ghost</PlayerName>
      </PlayerCardProvider>,
    );
    await user.click(screen.getByRole("button", { name: "Ghost" }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
