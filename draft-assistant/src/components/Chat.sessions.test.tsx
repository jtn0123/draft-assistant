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
  askClaude: vi.fn<(args: ChatRequest) => Promise<ChatReply>>(),
}));

vi.mock("../api", () => ({ api: mocks }));

import { Chat } from "./Chat";

function settings(): ChatSettings {
  return {
    has_key: true,
    key_hint: "····abcd",
    cli_available: false,
    provider: "api",
    models: ["Opus 5", "Fable 5"],
    efforts: {
      "Opus 5": ["Off", "Low", "Medium", "High", "xhigh", "Max"],
      "Fable 5": ["Low", "Medium", "High", "xhigh", "Max"],
    },
    notes: {},
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
    ...overrides,
  };
}

const panel = (screenName: "draft" | "season" = "draft") =>
  render(<Chat screen={screenName} contextNote="Sees this draft" onClose={() => undefined} />);

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
});

describe("saved chats", () => {
  it("files a conversation once the first answer lands", async () => {
    panel();
    await ask("Who should I take?", "The running back.");
    await waitFor(() => expect(listSessions("draft")).toHaveLength(1));
    const [saved] = listSessions("draft");
    expect(saved.title).toBe("Who should I take?");
    expect(saved.questions).toBe(1);
    expect(screen.getByRole("combobox", { name: "Saved chats" })).toHaveDisplayValue(
      /Who should I take\? · 1 question/,
    );
  });

  it("reopens the newest conversation when the panel comes back", async () => {
    const first = panel();
    await ask("Who should I take?", "The running back.");
    await waitFor(() => expect(listSessions("draft")).toHaveLength(1));
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
    await waitFor(() => expect(listSessions("draft")).toHaveLength(1));

    await userEvent.click(screen.getByRole("button", { name: "New" }));
    await userEvent.click(screen.getByRole("button", { name: "Fresh start" }));
    await ask("Second question", "Second answer.");
    await waitFor(() => expect(listSessions("draft")).toHaveLength(2));

    const older = listSessions("draft").find((s) => s.title === "First question");
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
    await waitFor(() => expect(listSessions("draft")).toHaveLength(1));

    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(listSessions("draft")).toEqual([]);
    expect(screen.queryByText("The running back.")).not.toBeInTheDocument();
    expect(screen.getByText(/who to take/)).toBeInTheDocument();
  });

  it("keeps the draft's chats out of the season's", async () => {
    const draft = panel("draft");
    await ask("Who should I take?", "The running back.");
    await waitFor(() => expect(listSessions("draft")).toHaveLength(1));
    draft.unmount();

    panel("season");
    expect(await screen.findByText(/who to start/)).toBeInTheDocument();
    expect(listSessions("season")).toEqual([]);
  });
});

describe("the spend cap", () => {
  it("counts what a conversation has cost", async () => {
    panel();
    mocks.askClaude.mockResolvedValue(
      // 200k in, 40k out on Opus 5 — $1.00 + $1.00.
      reply({ text: "An answer.", input_tokens: 200_000, output_tokens: 40_000 }),
    );
    const input = await screen.findByRole("textbox", { name: "Ask Claude" });
    await userEvent.type(input, "A question{Enter}");
    await screen.findByText("An answer.");
    expect(screen.getByText("$2.00 spent")).toBeInTheDocument();
  });

  it("stops asking once the cap is reached, and starts again when it is raised", async () => {
    panel();
    const cap = await screen.findByLabelText("Spend cap in dollars");
    await userEvent.clear(cap);
    await userEvent.type(cap, "1");
    mocks.askClaude.mockResolvedValue(
      reply({ text: "An answer.", input_tokens: 400_000, output_tokens: 0 }),
    );
    const input = screen.getByRole("textbox", { name: "Ask Claude" });
    await userEvent.type(input, "A question{Enter}");
    await screen.findByText("An answer.");

    expect(screen.getByText(/spent \$2\.00 of its \$1\.00 budget/)).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Ask Claude" })).toBeDisabled();

    await userEvent.clear(cap);
    await userEvent.type(cap, "9");
    expect(screen.getByRole("textbox", { name: "Ask Claude" })).toBeEnabled();
  });

  it("never stops when the cap is zero", async () => {
    panel();
    const cap = await screen.findByLabelText("Spend cap in dollars");
    await userEvent.clear(cap);
    await userEvent.type(cap, "0");
    mocks.askClaude.mockResolvedValue(
      reply({ text: "An answer.", input_tokens: 4_000_000, output_tokens: 0 }),
    );
    await userEvent.type(screen.getByRole("textbox", { name: "Ask Claude" }), "A question{Enter}");
    await screen.findByText("An answer.");
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
