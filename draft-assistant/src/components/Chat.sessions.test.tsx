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
    models: ["Opus 5", "Fable 5"],
    efforts: {
      "Opus 5": ["Off", "Low", "Medium", "High", "xhigh", "Max"],
      "Fable 5": ["Low", "Medium", "High", "xhigh", "Max"],
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
  const input = await screen.findByRole("textbox", { name: "Ask Claude" });
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

describe("the spend cap", () => {
  it("counts what the backend charged, not what the panel guesses", async () => {
    panel();
    mocks.askClaude.mockResolvedValue(
      reply({ text: "An answer.", input_tokens: 200_000, output_tokens: 40_000, cost_usd: 2 }),
    );
    const input = await screen.findByRole("textbox", { name: "Ask Claude" });
    await userEvent.type(input, "A question{Enter}");
    await screen.findByText("An answer.");
    expect(screen.getByText("$2.00 spent")).toBeInTheDocument();
  });

  it("charges nothing for an answer that came through Claude Code", async () => {
    panel();
    mocks.askClaude.mockResolvedValue(
      // A subscription paid for these tokens; the panel must not bill them.
      reply({
        text: "An answer.",
        provider: "claude_code",
        input_tokens: 4_000_000,
        output_tokens: 200_000,
        cost_usd: 0,
      }),
    );
    await userEvent.type(screen.getByRole("textbox", { name: "Ask Claude" }), "A question{Enter}");
    await screen.findByText("An answer.");
    expect(screen.getByText("$0.00 spent")).toBeInTheDocument();
    expect(screen.queryByText(/budget/)).not.toBeInTheDocument();
  });

  it("shows what the whole screen has spent, not just this conversation", async () => {
    // A conversation opened after earlier ones: the cap is checked against the
    // screen's total, so the panel has to show that total too.
    mocks.chatSettings.mockResolvedValue(settings({ spend_usd: { "draft.1": 3.5 } }));
    panel();
    await screen.findByRole("textbox", { name: "Ask Claude" });
    expect(screen.getByText(/\$3\.50 on this screen/)).toBeInTheDocument();

    mocks.askClaude.mockResolvedValue(
      reply({ text: "An answer.", cost_usd: 0.5, screen_spend_usd: 4 }),
    );
    await userEvent.type(screen.getByRole("textbox", { name: "Ask Claude" }), "A question{Enter}");
    await screen.findByText("An answer.");
    expect(screen.getByText(/\$0\.50 spent · \$4\.00 on this screen/)).toBeInTheDocument();
  });

  it("takes the cap from the backend, which is what enforces it", async () => {
    mocks.chatSettings.mockResolvedValue(settings({ budget_usd: 12 }));
    panel();
    expect(await screen.findByLabelText("Spend cap in dollars")).toHaveValue(12);
  });

  it("warns at the cap without taking the composer away", async () => {
    panel();
    const cap = await screen.findByLabelText("Spend cap in dollars");
    await userEvent.clear(cap);
    // Committed on Enter, not on every keystroke.
    await userEvent.type(cap, "1{Enter}");
    // The cap the backend enforces is kept in step with the one shown here.
    expect(mocks.setChatBudget).toHaveBeenLastCalledWith(1);

    mocks.askClaude.mockResolvedValue(reply({ text: "An answer.", cost_usd: 2 }));
    await userEvent.type(screen.getByRole("textbox", { name: "Ask Claude" }), "A question{Enter}");
    await screen.findByText("An answer.");

    expect(screen.getByText(/spent \$2\.00 of their \$1\.00 budget/)).toBeInTheDocument();
    // The backend is what refuses a turn; the panel only says it is coming.
    expect(screen.getByRole("textbox", { name: "Ask Claude" })).toBeEnabled();

    await userEvent.clear(cap);
    await userEvent.type(cap, "9{Enter}");
    expect(screen.queryByText(/of their \$1\.00 budget/)).not.toBeInTheDocument();
  });

  it("warns on what the screen has spent, not only on this conversation", async () => {
    // This chat has asked nothing; the cap the backend enforces has already
    // been reached by the conversations before it, and the next question here
    // is the one that gets refused.
    mocks.chatSettings.mockResolvedValue(settings({ budget_usd: 5, spend_usd: { "draft.1": 6 } }));
    panel();
    expect(
      await screen.findByText(/screen.s chats have spent \$6\.00 of their \$5\.00 budget/),
    ).toBeInTheDocument();
    // A warning, not a lock: the backend is the side that refuses.
    expect(screen.getByRole("textbox", { name: "Ask Claude" })).toBeEnabled();
  });

  it("does not read an emptied cap field as a cap of nothing", async () => {
    // 0 means "no cap at all", so a half-typed number must not be written as
    // one — the box being empty for a keystroke used to turn the cap off.
    mocks.chatSettings.mockResolvedValue(settings({ budget_usd: 5 }));
    panel();
    const cap = await screen.findByLabelText("Spend cap in dollars");
    mocks.setChatBudget.mockClear();

    await userEvent.clear(cap);
    expect(mocks.setChatBudget).not.toHaveBeenCalled();

    await userEvent.type(cap, "7{Enter}");
    expect(mocks.setChatBudget.mock.calls.flat()).toEqual([7]);
  });

  it("shows the backend's refusal as the answer to the turn that was refused", async () => {
    panel();
    mocks.askClaude.mockRejectedValue(
      new Error("Ask Claude has spent $5.00 of its $5.00 cap on the draft screen"),
    );
    await userEvent.type(screen.getByRole("textbox", { name: "Ask Claude" }), "A question{Enter}");
    expect(await screen.findByText(/of its \$5\.00 cap on the draft screen/)).toBeInTheDocument();
  });

  it("never warns when the cap is zero", async () => {
    panel();
    const cap = await screen.findByLabelText("Spend cap in dollars");
    await userEvent.clear(cap);
    await userEvent.type(cap, "0{Enter}");
    mocks.askClaude.mockResolvedValue(reply({ text: "An answer.", cost_usd: 20 }));
    await userEvent.type(screen.getByRole("textbox", { name: "Ask Claude" }), "A question{Enter}");
    await screen.findByText("An answer.");
    expect(screen.queryByText(/budget/)).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Ask Claude" })).toBeEnabled();
  });
});

describe("Markdown in an answer", () => {
  it("sets bold, lists and headings rather than showing the markup", async () => {
    panel();
    mocks.askClaude.mockResolvedValue(
      reply({ text: "## Verdict\n\nTake **Bijan**.\n\n- he scores\n- he plays" }),
    );
    const input = await screen.findByRole("textbox", { name: "Ask Claude" });
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

describe("the spend cap", () => {
  it("commits what was typed on blur rather than on every keystroke", async () => {
    // Every keystroke used to be written, so "12" set a cap of 1 on its way
    // to 12 — and each of those went to the backend as a separate cap.
    mocks.chatSettings.mockResolvedValue(settings({ budget_usd: 5 }));
    panel();
    const cap = await screen.findByLabelText("Spend cap in dollars");
    mocks.setChatBudget.mockClear();

    await userEvent.clear(cap);
    await userEvent.type(cap, "12");
    expect(mocks.setChatBudget).not.toHaveBeenCalled();

    await userEvent.tab();
    expect(mocks.setChatBudget.mock.calls.flat()).toEqual([12]);
  });

  it("refuses a negative cap and keeps the one that was in force", async () => {
    // 0 is the value that means "no cap", and a negative one used to be
    // rounded up to it — so "-1" removed the budget instead of being refused.
    mocks.chatSettings.mockResolvedValue(settings({ budget_usd: 5 }));
    panel();
    const cap = await screen.findByLabelText("Spend cap in dollars");
    mocks.setChatBudget.mockClear();

    await userEvent.clear(cap);
    await userEvent.type(cap, "-1{Enter}");

    expect(mocks.setChatBudget).not.toHaveBeenCalled();
    expect(cap).toHaveValue(5);
    expect(await screen.findByRole("alert")).toHaveTextContent(/not a budget/);
  });

  it("says nothing about a cap that is simply a number", async () => {
    mocks.chatSettings.mockResolvedValue(settings({ budget_usd: 5 }));
    panel();
    const cap = await screen.findByLabelText("Spend cap in dollars");
    await userEvent.clear(cap);
    await userEvent.type(cap, "8{Enter}");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(cap).toHaveValue(8);
  });

  it("counts what this league's chats spent, not another league's", async () => {
    // Conversations are filed per screen and league; the running spend the cap
    // is checked against is filed the same way. A bare screen key is from the
    // scheme before that and is a mixture of every league's spending.
    mocks.chatSettings.mockResolvedValue(
      settings({ spend_usd: { "draft.2": 4, "draft.9": 99, draft: 77 } }),
    );
    panel("draft", "2");
    await screen.findByRole("textbox", { name: "Ask Claude" });
    expect(screen.getByText(/\$4\.00 on this screen/)).toBeInTheDocument();
    expect(screen.queryByText(/\$99\.00|\$77\.00/)).not.toBeInTheDocument();
  });

  it("says who pays when the answers come from Claude Code", async () => {
    // The CLI route is billed to the subscription, so the panel sits at
    // "$0.00 spent" forever and the cap counts nothing. Unexplained, that
    // reads as a broken meter.
    mocks.chatSettings.mockResolvedValue(
      settings({ provider: "claude_code", cli_available: true }),
    );
    panel();
    expect(await screen.findByText(/billed to your Claude subscription/)).toBeInTheDocument();
  });

  it("says nothing of the sort on the API route, where the cap is real", async () => {
    panel();
    await screen.findByRole("textbox", { name: "Ask Claude" });
    expect(screen.queryByText(/billed to your Claude subscription/)).not.toBeInTheDocument();
  });
});
