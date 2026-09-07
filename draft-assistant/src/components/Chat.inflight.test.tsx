// The Ask Claude panel while an answer is on its way: the Cancel button, the
// cut-short answer it hands back, a question that outlives the panel that
// asked it, and the screen spend following the phones' questions.

import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatReply, ChatRequest, ChatSettings } from "../chat-types";
import { listSessions } from "../chatSessions";
import type { SharedChatThread } from "../types";

const mocks = vi.hoisted(() => ({
  chatSettings: vi.fn(),
  chatSuggestions: vi.fn(),
  setChatProvider: vi.fn(),
  setApiKey: vi.fn(),
  setChatBudget: vi.fn(),
  askClaude: vi.fn<(args: ChatRequest) => Promise<ChatReply>>(),
  onSharedChat: vi.fn(),
  cancelClaude: vi.fn<(screen: string) => Promise<boolean>>(),
}));

vi.mock("../api", () => ({ api: mocks }));
vi.mock("../chatCancel", () => ({ cancelClaude: mocks.cancelClaude }));

import { Chat } from "./Chat";
import { resetPendingTurns } from "../chatPending";

function settings(overrides: Partial<ChatSettings> = {}): ChatSettings {
  return {
    has_key: true,
    key_hint: "····abcd",
    cli_available: false,
    provider: "api",
    key_store: "keychain",
    budget_usd: 5,
    spend_usd: {},
    models: ["Opus 5", "Fable 5"],
    efforts: {
      "Opus 5": ["Off", "Low", "Medium", "High", "xhigh", "Max"],
      "Fable 5": ["Low", "Medium", "High", "xhigh", "Max"],
    },
    notes: {},
    ...overrides,
  };
}

/** A whole ChatReply, so a new field cannot go untested by accident. */
function reply(overrides: Partial<ChatReply>): ChatReply {
  return {
    text: "",
    thinking: null,
    model: "Opus 5",
    refused: false,
    truncated: false,
    cancelled: false,
    input_tokens: 0,
    output_tokens: 0,
    provider: "api",
    cost_usd: 0,
    screen_spend_usd: 0,
    ...overrides,
  };
}

/** An `askClaude` the test answers by hand, once it has decided how. */
function held() {
  let resolve!: (reply: ChatReply) => void;
  const answer = new Promise<ChatReply>((r) => {
    resolve = r;
  });
  mocks.askClaude.mockReturnValue(answer);
  return { resolve };
}

const panel = () =>
  render(
    <Chat screen="draft" leagueId="1" contextNote="Sees this draft" onClose={() => undefined} />,
  );

/** The handler the panel registered for `shared-chat`, once it has. */
let pushThread: ((next: SharedChatThread) => void) | null = null;

beforeEach(() => {
  vi.clearAllMocks();
  resetPendingTurns();
  pushThread = null;
  // jsdom has no scrollTo; the thread scrolls itself on every new turn.
  Element.prototype.scrollTo = vi.fn();
  mocks.chatSuggestions.mockResolvedValue([]);
  mocks.chatSettings.mockResolvedValue(settings());
  mocks.cancelClaude.mockResolvedValue(true);
  mocks.onSharedChat.mockImplementation((handler: (next: SharedChatThread) => void) => {
    pushThread = handler;
    return Promise.resolve(() => undefined);
  });
});

describe("an answer in flight", () => {
  it("offers Cancel while thinking and keeps the partial answer, marked cut short", async () => {
    const { resolve } = held();
    panel();
    const input = await screen.findByRole("textbox", { name: "Ask Claude" });
    await userEvent.type(input, "Who?{Enter}");

    await userEvent.click(await screen.findByRole("button", { name: "Cancel the answer" }));
    expect(mocks.cancelClaude).toHaveBeenCalledWith("draft");
    // The backend answers the same call with what had arrived by then.
    resolve(
      reply({
        text: "Take the RB, because",
        cancelled: true,
        cost_usd: 0.02,
        screen_spend_usd: 0.02,
      }),
    );
    expect(await screen.findByText("Cut short")).toBeInTheDocument();
    expect(screen.getByText("Take the RB, because")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Cancel the answer" })).not.toBeInTheDocument();
    // A cut-short turn is still a turn that cost something.
    expect(screen.getByText(/\$0\.02 spent/)).toBeInTheDocument();

    // And the partial answer is context for the follow-up.
    mocks.askClaude.mockResolvedValue(reply({ text: "Because he scores." }));
    await userEvent.type(input, "Go on{Enter}");
    await screen.findByText("Because he scores.");
    expect(mocks.askClaude.mock.calls[1]?.[0].messages).toEqual([
      { role: "user", content: "Who?" },
      { role: "assistant", content: "Take the RB, because" },
      { role: "user", content: "Go on" },
    ]);
  });

  it("reads a cancel with no text as a stopped turn, and does not resend it", async () => {
    const { resolve } = held();
    panel();
    const input = await screen.findByRole("textbox", { name: "Ask Claude" });
    await userEvent.type(input, "Who?{Enter}");
    await userEvent.click(await screen.findByRole("button", { name: "Cancel the answer" }));
    resolve(reply({ text: "", cancelled: true }));
    expect(await screen.findByText("Cancelled before an answer arrived.")).toBeInTheDocument();

    mocks.askClaude.mockResolvedValue(reply({ text: "Now." }));
    await userEvent.type(input, "Again{Enter}");
    await screen.findByText("Now.");
    expect(mocks.askClaude.mock.calls[1]?.[0].messages).toEqual([
      { role: "user", content: "Again" },
    ]);
  });

  it("keeps the question, the thinking state and the spend across a close and reopen", async () => {
    const { resolve } = held();
    const first = panel();
    const input = await screen.findByRole("textbox", { name: "Ask Claude" });
    await userEvent.type(input, "Who should I take?{Enter}");
    expect(screen.getByText(/Thinking it through/)).toBeInTheDocument();
    // Closing the panel unmounts it; the question used to go with it.
    first.unmount();

    panel();
    expect(await screen.findByText("Who should I take?")).toBeInTheDocument();
    expect(screen.getByText(/Thinking it through/)).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Ask Claude" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel the answer" })).toBeInTheDocument();

    resolve(reply({ text: "The RB.", cost_usd: 0.05, screen_spend_usd: 0.05 }));
    expect(await screen.findByText("The RB.")).toBeInTheDocument();
    expect(screen.getByText(/\$0\.05 spent/)).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Ask Claude" })).toBeEnabled();
    // Asked once: the reopened panel picked the turn up rather than resending.
    expect(mocks.askClaude).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(listSessions("draft.1")).toHaveLength(1));
    expect(listSessions("draft.1")[0]).toMatchObject({ questions: 1, costUsd: 0.05 });
  });

  it("files the answer into the conversation while the panel stays closed", async () => {
    const { resolve } = held();
    const first = panel();
    const input = await screen.findByRole("textbox", { name: "Ask Claude" });
    await userEvent.type(input, "Who should I take?{Enter}");
    first.unmount();

    resolve(reply({ text: "The RB.", cost_usd: 0.05, screen_spend_usd: 0.05 }));
    await waitFor(() => expect(listSessions("draft.1")).toHaveLength(1));
    expect(listSessions("draft.1")[0]).toMatchObject({
      title: "Who should I take?",
      questions: 1,
      costUsd: 0.05,
    });

    // Reopened later: the answer is there, and so is what it cost.
    panel();
    expect(await screen.findByText("The RB.")).toBeInTheDocument();
    expect(screen.getByText(/\$0\.05 spent/)).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Ask Claude" })).toBeEnabled();
  });
});

describe("the screen spend", () => {
  it("moves when a phone's question is answered on the shared thread", async () => {
    mocks.chatSettings
      .mockResolvedValueOnce(settings({ spend_usd: { "draft.1": 0.1 } }))
      .mockResolvedValue(settings({ spend_usd: { "draft.1": 0.75 } }));
    panel();
    await screen.findByRole("textbox", { name: "Ask Claude" });
    expect(screen.getByText(/\$0\.10 on this screen/)).toBeInTheDocument();
    await waitFor(() => expect(pushThread).not.toBeNull());

    // A question posted is not money spent yet, and the other screen's thread
    // is not this screen's tally.
    act(() => pushThread?.({ league_id: "1", screen: "draft", busy: true, entries: [] }));
    act(() => pushThread?.({ league_id: "1", screen: "season", busy: false, entries: [] }));
    expect(mocks.chatSettings).toHaveBeenCalledTimes(1);

    // The answer lands on the host's budget, and the figure follows it.
    act(() => pushThread?.({ league_id: "1", screen: "draft", busy: false, entries: [] }));
    expect(await screen.findByText(/\$0\.75 on this screen/)).toBeInTheDocument();
  });
});
