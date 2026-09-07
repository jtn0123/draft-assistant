// What the draft screen still lets the user do once the draft is complete:
// nothing that writes. The board's Draft buttons, the rec cards' "Mark
// drafted" and the header's Undo all stayed live after the final pick, so a
// stray click recorded a manual pick into, or undid one out of, a finished
// draft.

import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DraftView } from "./types";

vi.mock("./api", async () => ({ api: (await import("./test/appHarness")).harness().api }));

import "./test/warmScreens";
import App from "./App";
import { resetPrefs } from "./prefs";
import { draftFixture, fakeStorage, harness, restoringConfig } from "./test/appHarness";

const h = harness();

/** The dev fixture, after its last pick. */
function finished(): DraftView {
  const view = draftFixture(24);
  view.draft.status = "complete";
  view.draft.is_my_pick = false;
  view.draft.clock_deadline_ms = null;
  return view;
}

/** Every row's Draft button, once the lazy DraftScreen chunk has arrived.
 *  The first "Draft" button is the Draft/Season mode toggle. */
async function rowDraftButtons(): Promise<HTMLElement[]> {
  return waitFor(
    () => {
      const buttons = screen.getAllByRole("button", { name: "Draft" });
      expect(buttons.length).toBeGreaterThan(1);
      return buttons.slice(1);
    },
    { timeout: 5000 },
  );
}

beforeEach(() => {
  fakeStorage({ "da.screen": "draft" });
  resetPrefs();
  h.reset();
});

describe("once the draft is complete", () => {
  it("records nothing and undoes nothing", async () => {
    const user = userEvent.setup();
    const view = finished();
    h.api.getConfig.mockResolvedValue(restoringConfig(view));
    h.api.addLeague.mockResolvedValue(view);
    h.api.recordManualPick.mockResolvedValue(view);
    h.api.undoManualPick.mockResolvedValue(view);

    render(<App />);
    expect(await screen.findByRole("heading", { name: view.league.name })).toBeInTheDocument();

    // The board: every row's button is off, and a click on one goes nowhere.
    const rows = await rowDraftButtons();
    for (const button of rows) expect(button).toBeDisabled();
    await user.click(rows[0]);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    // The rec cards: the same call as a row, without a disabled button in
    // the way. It is refused with a toast rather than opening the dialog.
    const marks = screen.getAllByRole("button", { name: "Mark drafted" });
    expect(marks.length).toBeGreaterThan(0);
    await user.click(marks[0]);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByText(/The draft is complete: .* cannot be recorded/)).toBeInTheDocument();
    expect(h.api.recordManualPick).not.toHaveBeenCalled();

    // The header's Undo.
    await user.click(screen.getByRole("button", { name: "Undo" }));
    expect(h.api.undoManualPick).not.toHaveBeenCalled();
    expect(
      screen.getByText("The draft is complete: there is no recorded pick to undo"),
    ).toBeInTheDocument();
  });

  it("is a live board again the moment the draft is not complete", async () => {
    // The gate reads the view, not a flag set once: a status that goes back
    // to drafting (a commissioner reopening the draft) brings the buttons back.
    const user = userEvent.setup();
    const view = finished();
    const reopened = draftFixture(24);
    h.api.getConfig.mockResolvedValue(restoringConfig(view));
    h.api.addLeague.mockResolvedValue(view);
    h.api.undoManualPick.mockResolvedValue(reopened);

    render(<App />);
    expect(await screen.findByRole("heading", { name: view.league.name })).toBeInTheDocument();
    for (const button of await rowDraftButtons()) expect(button).toBeDisabled();

    act(() => h.push.draft?.(reopened));
    for (const button of await rowDraftButtons()) expect(button).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Undo" }));
    expect(h.api.undoManualPick).toHaveBeenCalledTimes(1);
  });
});
