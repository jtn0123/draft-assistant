import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatSettings } from "../chat-types";

const mocks = vi.hoisted(() => ({
  chatSettings: vi.fn(),
  chatSuggestions: vi.fn(),
  setChatProvider: vi.fn(),
  setApiKey: vi.fn(),
  askClaude: vi.fn(),
}));

vi.mock("../api", () => ({ api: mocks }));

import { Chat } from "./Chat";

function settings(overrides: Partial<ChatSettings>): ChatSettings {
  return {
    has_key: false,
    key_hint: null,
    cli_available: false,
    provider: "api",
    models: ["Opus 5", "Fable 5"],
    efforts: {
      "Opus 5": ["Off", "Low", "Medium", "High", "xhigh", "Max"],
      "Fable 5": ["Low", "Medium", "High", "xhigh", "Max"],
    },
    notes: {},
    ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  // jsdom has no scrollTo; the thread scrolls itself on every new turn.
  Element.prototype.scrollTo = vi.fn();
  mocks.chatSuggestions.mockResolvedValue([]);
});

describe("Chat copy", () => {
  it("names the model and effort in the context line and tips each model", async () => {
    mocks.chatSettings.mockResolvedValue(settings({}));
    render(<Chat screen="draft" contextNote="Sees this draft · pick 3.04" onClose={() => undefined} />);
    await waitFor(() => expect(screen.getByLabelText("Anthropic API key")).toBeInTheDocument());
    expect(screen.getByText(/Sees this draft · pick 3\.04 · Opus 5 · high effort/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Opus 5" })).toHaveAttribute("title");
    expect(screen.getByRole("button", { name: "Fable 5" })).toHaveAttribute("title");
  });

  it("changes the empty-thread copy for the season screen", async () => {
    mocks.chatSettings.mockResolvedValue(settings({ has_key: true, key_hint: "····abcd" }));
    render(<Chat screen="season" contextNote="Sees week 1" onClose={() => undefined} />);
    await waitFor(() => expect(screen.getByText(/who to start/)).toBeInTheDocument());
  });
});

describe("Chat routing", () => {
  it("asks for a key only when the API route has none", async () => {
    mocks.chatSettings.mockResolvedValue(settings({ cli_available: true, provider: "claude_code" }));
    render(<Chat screen="draft" contextNote="Sees this draft" onClose={() => undefined} />);
    await waitFor(() => expect(screen.getByRole("group", { name: "Route" })).toBeInTheDocument());
    expect(screen.queryByLabelText("Anthropic API key")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Ask Claude" })).toBeEnabled();
    expect(screen.getByText(/via Claude Code/)).toBeInTheDocument();
  });

  it("hides the route picker when the CLI is not installed", async () => {
    mocks.chatSettings.mockResolvedValue(settings({}));
    render(<Chat screen="draft" contextNote="Sees this draft" onClose={() => undefined} />);
    await waitFor(() => expect(screen.getByLabelText("Anthropic API key")).toBeInTheDocument());
    expect(screen.queryByRole("group", { name: "Route" })).not.toBeInTheDocument();
  });

  it("switching route saves it and re-reads the settings", async () => {
    mocks.chatSettings
      .mockResolvedValueOnce(settings({ cli_available: true, provider: "claude_code" }))
      .mockResolvedValueOnce(settings({ cli_available: true, provider: "api" }));
    mocks.setChatProvider.mockResolvedValue("api");
    render(<Chat screen="draft" contextNote="Sees this draft" onClose={() => undefined} />);
    await waitFor(() => expect(screen.getByRole("group", { name: "Route" })).toBeInTheDocument());

    await userEvent.click(screen.getByRole("button", { name: "API key" }));
    expect(mocks.setChatProvider).toHaveBeenCalledWith("api");
    await waitFor(() => expect(screen.getByLabelText("Anthropic API key")).toBeInTheDocument());
    expect(screen.getByText(/via the API/)).toBeInTheDocument();
  });
});

describe("Chat conversation", () => {
  it("sends a question, shows the answer, and keeps the thread as history", async () => {
    mocks.chatSettings.mockResolvedValue(settings({ has_key: true, key_hint: "····abcd" }));
    mocks.askClaude.mockResolvedValue({ refused: false, text: "Take the RB.\n\nHe scores more." });
    render(<Chat screen="draft" contextNote="Sees this draft" onClose={() => undefined} />);
    const input = await screen.findByRole("textbox", { name: "Ask Claude" });

    await userEvent.type(input, "Who should I take?{Enter}");
    expect(await screen.findByText("Take the RB.")).toBeInTheDocument();
    expect(screen.getByText("He scores more.")).toBeInTheDocument();
    expect(screen.getByText("Who should I take?")).toBeInTheDocument();
    expect(mocks.askClaude).toHaveBeenCalledWith({
      screen: "draft",
      model: "Opus 5",
      effort: "High",
      messages: [{ role: "user", content: "Who should I take?" }],
    });

    // The follow-up carries the whole thread.
    await userEvent.type(input, "Why him?{Enter}");
    await waitFor(() => expect(mocks.askClaude).toHaveBeenCalledTimes(2));
    expect(mocks.askClaude.mock.calls[1]?.[0].messages).toHaveLength(3);
  });

  it("labels a refusal", async () => {
    mocks.chatSettings.mockResolvedValue(settings({ has_key: true }));
    mocks.askClaude.mockResolvedValue({ refused: true, text: "I can't help with that." });
    render(<Chat screen="draft" contextNote="Sees this draft" onClose={() => undefined} />);
    const input = await screen.findByRole("textbox", { name: "Ask Claude" });
    await userEvent.type(input, "collude with me{Enter}");
    expect(await screen.findByText("Declined")).toBeInTheDocument();
  });

  it("shows an error turn and drops the failed question from history", async () => {
    mocks.chatSettings.mockResolvedValue(settings({ has_key: true }));
    mocks.askClaude.mockRejectedValueOnce(new Error("network down")).mockResolvedValue({
      refused: false,
      text: "Back online.",
    });
    render(<Chat screen="draft" contextNote="Sees this draft" onClose={() => undefined} />);
    const input = await screen.findByRole("textbox", { name: "Ask Claude" });

    await userEvent.type(input, "First try{Enter}");
    expect(await screen.findByText(/network down/)).toBeInTheDocument();

    await userEvent.type(input, "Second try{Enter}");
    await screen.findByText("Back online.");
    // The retry must not resend the failed turn.
    expect(mocks.askClaude.mock.calls[1]?.[0].messages).toEqual([
      { role: "user", content: "Second try" },
    ]);
  });

  it("asks a suggestion with one click", async () => {
    mocks.chatSettings.mockResolvedValue(settings({ has_key: true }));
    mocks.chatSuggestions.mockResolvedValue(["Who do I start this week?"]);
    mocks.askClaude.mockResolvedValue({ refused: false, text: "Start Downs." });
    render(<Chat screen="season" contextNote="Sees week 1" onClose={() => undefined} />);
    await userEvent.click(await screen.findByRole("button", { name: "Who do I start this week?" }));
    expect(await screen.findByText("Start Downs.")).toBeInTheDocument();
    expect(mocks.chatSuggestions).toHaveBeenCalledWith("season");
  });
});

describe("Chat model and effort", () => {
  it("falls back to a legal effort when the picked model cannot turn thinking off", async () => {
    mocks.chatSettings.mockResolvedValue(settings({ has_key: true }));
    render(<Chat screen="draft" contextNote="Sees this draft" onClose={() => undefined} />);
    await screen.findByRole("textbox", { name: "Ask Claude" });

    await userEvent.click(screen.getByRole("button", { name: "Off" }));
    expect(screen.getByText(/no thinking/)).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Fable 5" }));
    expect(screen.queryByRole("button", { name: "Off" })).not.toBeInTheDocument();
    expect(screen.getByText(/Fable 5 · high effort/)).toBeInTheDocument();
    expect(screen.getByText("thinking always on")).toBeInTheDocument();
  });
});

describe("Chat thread controls", () => {
  const startThread = async () => {
    mocks.chatSettings.mockResolvedValue(settings({ has_key: true }));
    mocks.askClaude.mockResolvedValue({ refused: false, text: "An answer." });
    render(<Chat screen="draft" contextNote="Sees this draft" onClose={() => undefined} />);
    const input = await screen.findByRole("textbox", { name: "Ask Claude" });
    await userEvent.type(input, "A question{Enter}");
    await screen.findByText("An answer.");
  };

  it("Fresh start wipes the thread and the history", async () => {
    await startThread();
    await userEvent.click(screen.getByRole("button", { name: "New" }));
    await userEvent.click(screen.getByRole("button", { name: "Fresh start" }));
    expect(screen.queryByText("An answer.")).not.toBeInTheDocument();
    expect(screen.getByText(/who to take/)).toBeInTheDocument();
  });

  it("Carry this thread keeps the turns and marks the seam", async () => {
    await startThread();
    await userEvent.click(screen.getByRole("button", { name: "New" }));
    await userEvent.click(screen.getByRole("button", { name: "Carry this thread" }));
    expect(screen.getByText("An answer.")).toBeInTheDocument();
    expect(screen.getByText(/carried the thread above as context/)).toBeInTheDocument();
  });

  it("Cancel dismisses the new-chat bar", async () => {
    await startThread();
    await userEvent.click(screen.getByRole("button", { name: "New" }));
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByText(/Start a new chat/)).not.toBeInTheDocument();
  });

  it("toggles compact spacing", async () => {
    mocks.chatSettings.mockResolvedValue(settings({ has_key: true }));
    const { container } = render(
      <Chat screen="draft" contextNote="Sees this draft" onClose={() => undefined} />,
    );
    await screen.findByRole("textbox", { name: "Ask Claude" });
    await userEvent.click(screen.getByRole("button", { name: "Compact" }));
    expect(container.querySelector(".chat-thread.is-compact")).not.toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Cozy" }));
    expect(container.querySelector(".chat-thread.is-compact")).toBeNull();
  });
});

describe("API key form", () => {
  it("saves a key and unlocks the composer", async () => {
    mocks.chatSettings
      .mockResolvedValueOnce(settings({}))
      .mockResolvedValueOnce(settings({ has_key: true, key_hint: "····wxyz" }));
    mocks.setApiKey.mockResolvedValue(true);
    render(<Chat screen="draft" contextNote="Sees this draft" onClose={() => undefined} />);
    const field = await screen.findByLabelText("Anthropic API key");
    expect(screen.getByRole("textbox", { name: "Ask Claude" })).toBeDisabled();

    await userEvent.type(field, "sk-ant-test{Enter}");
    expect(mocks.setApiKey).toHaveBeenCalledWith("sk-ant-test");
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Ask Claude" })).toBeEnabled(),
    );
  });

  it("surfaces a save failure without losing the form", async () => {
    mocks.chatSettings.mockResolvedValue(settings({}));
    mocks.setApiKey.mockRejectedValue(new Error("keychain denied"));
    render(<Chat screen="draft" contextNote="Sees this draft" onClose={() => undefined} />);
    const field = await screen.findByLabelText("Anthropic API key");
    await userEvent.type(field, "sk-ant-test");
    await userEvent.click(screen.getByRole("button", { name: "Save key" }));
    expect(await screen.findByText(/keychain denied/)).toBeInTheDocument();
    expect(screen.getByLabelText("Anthropic API key")).toBeInTheDocument();
  });

  it("offers to change a stored key and shows the hint", async () => {
    mocks.chatSettings.mockResolvedValue(settings({ has_key: true, key_hint: "····abcd" }));
    render(<Chat screen="draft" contextNote="Sees this draft" onClose={() => undefined} />);
    await screen.findByRole("textbox", { name: "Ask Claude" });
    await userEvent.click(screen.getByRole("button", { name: "change key" }));
    expect(screen.getByText("Replace the stored key")).toBeInTheDocument();
    expect(screen.getByText(/Currently using ····abcd/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "cancel" }));
    expect(screen.queryByText("Replace the stored key")).not.toBeInTheDocument();
  });
});
