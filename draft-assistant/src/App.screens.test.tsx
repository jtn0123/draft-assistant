import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import fixtureJson from "../public/dev-fixture.json";
import type { DraftView } from "./types";

// The same api mock as App.test.tsx, trimmed to what switching screens needs.
const testState = vi.hoisted(() => ({
  api: {
    addLeague: vi.fn(),
    getConfig: vi.fn(),
    startPolling: vi.fn(),
    stopPolling: vi.fn(),
    listChatSessions: vi.fn(),
    saveChatSession: vi.fn(),
    onDraftUpdated: vi.fn(),
    onPollHealth: vi.fn(),
  },
}));

vi.mock("./api", () => ({ api: testState.api }));

import App from "./App";

let nextSeq = 0;

function fixture(): DraftView {
  const view = structuredClone(fixtureJson) as unknown as DraftView;
  view.seq = ++nextSeq;
  view.available = view.available.slice(0, 40);
  return view;
}

beforeEach(() => {
  vi.clearAllMocks();
  window.localStorage.clear();
  nextSeq = 0;
  testState.api.startPolling.mockResolvedValue(undefined);
  testState.api.stopPolling.mockResolvedValue(undefined);
  testState.api.listChatSessions.mockResolvedValue([]);
  testState.api.saveChatSession.mockResolvedValue("");
  testState.api.onDraftUpdated.mockResolvedValue(() => undefined);
  testState.api.onPollHealth.mockResolvedValue(() => undefined);
});

describe("draft and season screens", () => {
  function load(initial: DraftView) {
    testState.api.getConfig.mockResolvedValue({
      my_user_id: "browser-preview",
      active_league_id: initial.league.league_id,
      leagues: [],
    });
    testState.api.addLeague.mockResolvedValue(initial);
  }

  it("shows the draft while it is on, with the season a switch away", async () => {
    const user = userEvent.setup();
    load(fixture());
    render(<App />);
    await screen.findByText("YOU ARE ON THE CLOCK");
    expect(screen.getByRole("button", { name: "Draft screen" })).toHaveAttribute("aria-pressed", "true");

    await user.click(screen.getByRole("button", { name: "Season screen" }));
    expect(screen.queryByText("YOU ARE ON THE CLOCK")).not.toBeInTheDocument();
    expect(screen.getByText(/No week on the calendar yet/)).toBeInTheDocument();
    // Draft-only controls leave with the draft.
    expect(screen.queryByRole("button", { name: "Undo" })).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Toggle pick numbering")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 2, name: "My roster" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Tier alerts" })).not.toBeInTheDocument();
  });

  it("opens on the season once the draft is complete and remembers a switch back", async () => {
    const user = userEvent.setup();
    const initial = fixture();
    initial.draft.status = "complete";
    load(initial);
    render(<App />);
    await screen.findByText(initial.league.name);
    expect(screen.getByRole("button", { name: "Season screen" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.queryByText("YOU ARE ON THE CLOCK")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Draft screen" }));
    expect(screen.getByRole("button", { name: "Undo" })).toBeInTheDocument();
    expect(
      window.localStorage.getItem(`draft-assistant.view-mode:${initial.draft.draft_id}`),
    ).toBe("draft");
  });
});
