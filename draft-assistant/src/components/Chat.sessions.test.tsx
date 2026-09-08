import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatReply, ChatRequest, ChatSettings } from "../chat-types";
import { listSessions } from "../chatSessions";

const mocks = vi.hoisted(() => ({
  chatSettings: vi.fn(),
  chatSuggestions: vi.fn(),
  setChatProvider: vi.fn(),
  setApiKey: vi.fn(),
  setChatBudget: vi.fn(),
  askClaude: vi.fn<(args: ChatRequest) => Promise<ChatReply>>(),
  onSharedChat: vi.fn(() => Promise.resolve(() => undefined)),
  onChatProgress: vi.fn(() => Promise.resolve(() => undefined)),
}));

vi.mock("../api", () => ({ api: mocks }));

import { Chat } from "./Chat";

function settings(overrides: Partial<ChatSettings> = {}): ChatSettings {
  return {
    has_key: true,
    key_hint: "····abcd",
    cli_available: false,
    provider: "api",
    key_store: "keychain",
    budget_usd: 5,
    spend_usd: {},
    models: ["Opus 5", "Fable 5.1"],
    efforts: {
      "Opus 5": ["Off", "Low", "Medium", "High", "xhigh", "Max"],
      "Fable 5.1": ["Low", "Medium", "High", "xhigh", "Max"],
    },
    notes: {},
    ...overrides,
  };
}

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

const panel = (screenName: "draft" | "season" = "draft", leagueId = "1") =>
  render(
    <Chat
      screen={screenName}
      leagueId={leagueId}
      contextNote="Sees this draft"
      onClose={() => undefined}
    />,
  );

async function ask(question: string, answer: string) {
  mocks.askClaude.mockResolvedValue(reply({ text: answer }));
  const input = await screen.findByRole("textbox", { name: "Ask AI" });
  await userEvent.type(input, `${question}{Enter}`);
  await screen.findByText(answer);
}

beforeEach(() => {
  vi.clearAllMocks();
  Element.prototype.scrollTo = vi.fn();
  mocks.chatSuggestions.mockResolvedValue([]);
  mocks.chatSettings.mockResolvedValue(settings());
  mocks.setChatBudget.mockImplementation((dollars: number) => Promise.resolve(dollars));
});

describe("saved chats", () => {
  it("files a conversation once the first answer lands", async () => {
    panel();
    await ask("Who should I take?", "The running back.");
    await waitFor(() => expect(listSessions("draft.1")).toHaveLength(1));
    const [saved] = listSessions("draft.1");
    expect(saved.title).toBe("Who should I take?");
    expect(saved.questions).toBe(1);
    expect(screen.getByRole("combobox", { name: "Saved chats" })).toHaveDisplayValue(
      /Who should I take\? · 1 question/,
    );
  });

  it("reopens the newest conversation when the panel comes back", async () => {
    const first = panel();
    await ask("Who should I take?", "The running back.");
    await waitFor(() => expect(listSessions("draft.1")).toHaveLength(1));
    first.unmount();

    panel();
    expect(await screen.findByText("The running back.")).toBeInTheDocument();
    // The reopened thread is context for the next question, not a fresh start.
    await ask("Why him?", "More points.");
    expect(mocks.askClaude.mock.calls[1]?.[0].messages).toHaveLength(3);
  });

  it("switches between two saved conversations", async () => {
    panel();
    await ask("First question", "First answer.");
    await waitFor(() => expect(listSessions("draft.1")).toHaveLength(1));

    await userEvent.click(screen.getByRole("button", { name: "New" }));
    await userEvent.click(screen.getByRole("button", { name: "Fresh start" }));
    await ask("Second question", "Second answer.");
    await waitFor(() => expect(listSessions("draft.1")).toHaveLength(2));

    const older = listSessions("draft.1").find((s) => s.title === "First question");
    await userEvent.selectOptions(
      screen.getByRole("combobox", { name: "Saved chats" }),
      older?.id ?? "",
    );
    expect(await screen.findByText("First answer.")).toBeInTheDocument();
    expect(screen.queryByText("Second answer.")).not.toBeInTheDocument();
  });

  it("deleting the open conversation forgets it and empties the thread", async () => {
    panel();
    await ask("Who should I take?", "The running back.");
    await waitFor(() => expect(listSessions("draft.1")).toHaveLength(1));

    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(listSessions("draft.1")).toEqual([]);
    expect(screen.queryByText("The running back.")).not.toBeInTheDocument();
    expect(screen.getByText(/who to take/)).toBeInTheDocument();
  });

  it("keeps one league's chats out of another's", async () => {
    // The board a question was asked about is gone the moment the user
    // switches leagues; carrying the thread across would answer about players
    // who are not in this draft.
    const first = panel("draft", "1");
    await ask("Who should I take?", "The running back.");
    await waitFor(() => expect(listSessions("draft.1")).toHaveLength(1));
    first.unmount();

    panel("draft", "2");
    expect(await screen.findByText(/who to take/)).toBeInTheDocument();
    expect(screen.queryByText("The running back.")).not.toBeInTheDocument();
    expect(listSessions("draft.2")).toEqual([]);
    // …and the first league still has its own, waiting where it was left.
    expect(listSessions("draft.1")).toHaveLength(1);
  });

  it("keeps the draft's chats out of the season's", async () => {
    const draft = panel("draft");
    await ask("Who should I take?", "The running back.");
    await waitFor(() => expect(listSessions("draft.1")).toHaveLength(1));
    draft.unmount();

    panel("season");
    expect(await screen.findByText(/who to start/)).toBeInTheDocument();
    expect(listSessions("season.1")).toEqual([]);
  });
});

describe("estimated usage cost without a cap", () => {
  it("shows API-equivalent subscription cost and never writes a cap", async () => {
    mocks.chatSettings.mockResolvedValue(
      settings({ provider: "claude_code", cli_available: true, budget_usd: 1 }),
    );
    panel();
    mocks.askClaude.mockResolvedValue(
      reply({ text: "An answer.", provider: "claude_code", cost_usd: 20, screen_spend_usd: 25 }),
    );
    await userEvent.type(await screen.findByRole("textbox", { name: "Ask AI" }), "Who?{Enter}");
    await screen.findByText("An answer.");
    expect(screen.getByText(/\$20.00 estimated cost/)).toBeInTheDocument();
    expect(screen.getByText(/\$25.00 on this screen/)).toBeInTheDocument();
    expect(screen.getByText(/Subscription calls are not extra token bills/)).toBeInTheDocument();
    expect(screen.queryByLabelText("Spend cap in dollars")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Ask AI" })).toBeEnabled();
    expect(mocks.setChatBudget).not.toHaveBeenCalled();
  });
  it("keeps cost totals scoped to the selected league", async () => {
    mocks.chatSettings.mockResolvedValue(
      settings({ spend_usd: { "draft.2": 4, "draft.9": 99, draft: 77 } }),
    );
    panel("draft", "2");
    expect(await screen.findByText(/\$4.00 on this screen/)).toBeInTheDocument();
    expect(screen.queryByText(/\$99.00|\$77.00/)).not.toBeInTheDocument();
  });
});

describe("Markdown in an answer", () => {
  it("sets bold, lists and headings rather than showing the markup", async () => {
    panel();
    mocks.askClaude.mockResolvedValue(
      reply({ text: "## Verdict\n\nTake **Bijan**.\n\n- he scores\n- he plays" }),
    );
    const input = await screen.findByRole("textbox", { name: "Ask AI" });
    await userEvent.type(input, "Who?{Enter}");

    expect(await screen.findByText("Bijan")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Verdict" })).toBeInTheDocument();
    expect(screen.getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "he scores",
      "he plays",
    ]);
    expect(screen.queryByText(/\*\*Bijan\*\*/)).not.toBeInTheDocument();
  });
});
