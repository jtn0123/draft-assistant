import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatReply, ChatSession, ChatSessionSummary, ChatUsage } from "../types";

const testState = vi.hoisted(() => ({
  api: {
    chat: vi.fn(),
    chatCompact: vi.fn(),
    saveChatSession: vi.fn(),
    listChatSessions: vi.fn(),
    loadChatSession: vi.fn(),
  },
}));

vi.mock("../api", () => ({ api: testState.api }));

import { Chat } from "./Chat";

function usage(over: Partial<ChatUsage> = {}): ChatUsage {
  return {
    model: "opus",
    input_tokens: 12000,
    cache_read_tokens: 15000,
    cache_write_tokens: 3000,
    output_tokens: 80,
    context_tokens: 30000,
    web_searches: 0,
    duration_ms: 9120,
    cost_usd: 0.31,
    fast_mode: null,
    fast_mode_reason: null,
    ...over,
  };
}

function reply(answer: string, pick = 12): ChatReply {
  return { answer, usage: usage(), as_of: { pick, seq: 1 } };
}

function saved(id: string, title: string, updatedAt: number): ChatSession {
  return {
    id,
    draft_id: "d1",
    league_name: "Test league",
    started_at: updatedAt - 60,
    updated_at: updatedAt,
    title,
    turns: [
      { role: "you", text: title, as_of_pick: null },
      { role: "claude", text: `Answer to: ${title}`, as_of_pick: 3 },
    ],
    questions: 1,
    cost_usd: 0.2,
  };
}

const summary = (s: ChatSession): ChatSessionSummary => ({
  id: s.id,
  title: s.title,
  started_at: s.started_at,
  updated_at: s.updated_at,
  questions: s.questions,
  cost_usd: s.cost_usd,
});

const question = () => screen.getByLabelText("Your question");
const askButton = () => screen.getByRole("button", { name: "Ask" });
const sessionSelect = () => screen.getByLabelText("Saved sessions");
const panel = () => screen.getByRole("complementary", { name: /Ask Claude/ });

beforeEach(() => {
  vi.clearAllMocks();
  window.localStorage.clear();
  testState.api.listChatSessions.mockResolvedValue([]);
  testState.api.saveChatSession.mockResolvedValue("/data/chats/d1/x.json");
});

describe("Chat panel — saved sessions", () => {
  it("saves the conversation after each answer, filed under the draft", async () => {
    const user = userEvent.setup();
    testState.api.chat.mockResolvedValue(reply("Take Chris Olave."));
    render(<Chat open onClose={() => undefined} draftId="d1" leagueName="Test league" currentPick={12} />);

    expect(sessionSelect()).toHaveDisplayValue("Current chat (not saved yet)");
    await user.type(question(), "Who should I take?");
    await user.click(askButton());
    await waitFor(() => expect(testState.api.saveChatSession).toHaveBeenCalledTimes(1));

    const session = testState.api.saveChatSession.mock.calls[0][0] as ChatSession;
    expect(session.draft_id).toBe("d1");
    expect(session.league_name).toBe("Test league");
    expect(session.title).toBe("Who should I take?");
    expect(session.questions).toBe(1);
    expect(session.cost_usd).toBeCloseTo(0.31);
    expect(session.turns).toEqual([
      { role: "you", text: "Who should I take?", as_of_pick: null },
      { role: "claude", text: "Take Chris Olave.", as_of_pick: 12 },
    ]);
    expect(session.id).toMatch(/^\d+-[0-9a-f]{4}$/);
    await waitFor(() => expect(screen.getByText("saved")).toBeInTheDocument());
    expect(sessionSelect()).toHaveAttribute("title", "Saved to /data/chats/d1/x.json");

    // The follow-up rewrites the same file with the whole conversation.
    testState.api.chat.mockResolvedValue(reply("Because he is open."));
    await user.type(question(), "Why?");
    await user.click(askButton());
    await waitFor(() => expect(testState.api.saveChatSession).toHaveBeenCalledTimes(2));
    const again = testState.api.saveChatSession.mock.calls[1][0] as ChatSession;
    expect(again.id).toBe(session.id);
    expect(again.turns).toHaveLength(4);
    expect(again.questions).toBe(2);
  });

  it("reopens the most recent saved session when the panel first sees the draft", async () => {
    const older = saved("s-old", "Older question", 1000);
    const newer = saved("s-new", "Newer question", 2000);
    testState.api.listChatSessions.mockResolvedValue([summary(newer), summary(older)]);
    testState.api.loadChatSession.mockImplementation(async (_d: string, id: string) =>
      id === "s-new" ? newer : older,
    );
    render(<Chat open onClose={() => undefined} draftId="d1" currentPick={5} />);

    await waitFor(() => expect(testState.api.loadChatSession).toHaveBeenCalledWith("d1", "s-new"));
    const answers = await within(panel()).findAllByText(/Answer to:/);
    expect(answers[0]).toHaveTextContent("Answer to: Newer question");
    expect(within(panel()).getByText(/as of pick 3/)).toHaveTextContent("2 picks since");
    expect(sessionSelect()).toHaveDisplayValue(/Newer question/);
    // Reopening is not a change: nothing is written back.
    expect(testState.api.saveChatSession).not.toHaveBeenCalled();

    // Picking the other one swaps the conversation in place.
    const user = userEvent.setup();
    await user.selectOptions(sessionSelect(), "s-old");
    await waitFor(() =>
      expect(within(panel()).getByText(/Answer to:/)).toHaveTextContent("Answer to: Older question"),
    );
    expect(testState.api.saveChatSession).not.toHaveBeenCalled();
  });

  it("New chat starts a separate session and leaves the old one on disk", async () => {
    const user = userEvent.setup();
    const first = saved("s-1", "First", 1000);
    testState.api.listChatSessions.mockResolvedValue([summary(first)]);
    testState.api.loadChatSession.mockResolvedValue(first);
    render(<Chat open onClose={() => undefined} draftId="d1" currentPick={5} />);
    await within(panel()).findByText("Answer to: First");

    await user.click(screen.getByRole("button", { name: "New chat" }));
    expect(within(panel()).queryByText("Answer to: First")).toBeNull();
    expect(sessionSelect()).toHaveDisplayValue("Current chat (not saved yet)");

    testState.api.chat.mockResolvedValue(reply("Fresh answer."));
    await user.type(question(), "Second");
    await user.click(askButton());
    await waitFor(() => expect(testState.api.saveChatSession).toHaveBeenCalledTimes(1));
    const session = testState.api.saveChatSession.mock.calls[0][0] as ChatSession;
    expect(session.id).not.toBe("s-1");
    expect(session.title).toBe("Second");
    expect(session.turns).toHaveLength(2);
  });

  it("a save that fails is reported in the panel and the answer is kept", async () => {
    const user = userEvent.setup();
    testState.api.chat.mockResolvedValue(reply("Take Chris Olave."));
    testState.api.saveChatSession.mockRejectedValue("write /data/chats: disk full");
    render(<Chat open onClose={() => undefined} draftId="d1" />);
    await user.type(question(), "Who?");
    await user.click(askButton());
    const alert = await within(panel()).findByRole("alert");
    expect(alert).toHaveTextContent("Could not save this chat: write /data/chats: disk full");
    expect(within(panel()).getByText("Take Chris Olave.")).toBeInTheDocument();
  });

  it("without a draft nothing is listed or saved", async () => {
    const user = userEvent.setup();
    testState.api.chat.mockResolvedValue(reply("Answer."));
    render(<Chat open onClose={() => undefined} />);
    expect(screen.queryByLabelText("Saved sessions")).toBeNull();
    await user.type(question(), "Who?");
    await user.click(askButton());
    await within(panel()).findByText("Answer.");
    expect(testState.api.listChatSessions).not.toHaveBeenCalled();
    expect(testState.api.saveChatSession).not.toHaveBeenCalled();
  });
});

describe("Chat panel — the saved label", () => {
  it("says a reopened session is saved, not 'not saved yet'", async () => {
    // Found by driving the real window: reopening a session from disk left
    // the label claiming it had never been written.
    const stored = saved("s-1", "Earlier", 1000);
    testState.api.listChatSessions.mockResolvedValue([summary(stored)]);
    testState.api.loadChatSession.mockResolvedValue(stored);
    render(<Chat open onClose={() => undefined} draftId="d1" />);

    await within(panel()).findByText("Answer to: Earlier");
    expect(within(panel()).getByText("saved")).toBeInTheDocument();
    expect(within(panel()).queryByText("not saved yet")).toBeNull();
    expect(sessionSelect()).toHaveAttribute("title", "Saved");

    // New chat has nothing on disk yet, and says so.
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "New chat" }));
    expect(within(panel()).getByText("not saved yet")).toBeInTheDocument();
  });
});
