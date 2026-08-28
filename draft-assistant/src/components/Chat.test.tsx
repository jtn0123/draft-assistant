import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatReply, ChatUsage } from "../types";

const testState = vi.hoisted(() => ({
  api: { chat: vi.fn(), chatCompact: vi.fn() },
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

function reply(answer: string, over: Partial<ChatUsage> = {}): ChatReply {
  return { answer, usage: usage(over) };
}

/** A chat call that stays pending until the test releases it. */
function pendingAnswer(): (value: string) => void {
  let release: (value: ChatReply) => void = () => undefined;
  testState.api.chat.mockReturnValue(
    new Promise<ChatReply>((resolve) => {
      release = resolve;
    }),
  );
  return (value) => release(reply(value));
}

const question = () => screen.getByLabelText("Your question");
const askButton = () => screen.getByRole("button", { name: "Ask" });

beforeEach(() => {
  vi.clearAllMocks();
  window.localStorage.clear();
});

describe("Chat panel", () => {
  it("renders nothing while closed", () => {
    const { container } = render(<Chat open={false} onClose={() => undefined} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("sends a question with the conversation so far and shows the answer", async () => {
    const user = userEvent.setup();
    testState.api.chat.mockResolvedValue(reply("Take Chris Olave."));

    render(<Chat open onClose={() => undefined} />);
    await user.type(question(), "Who should I take?");
    await user.click(askButton());

    // The first question carries no history and the default settings.
    expect(testState.api.chat).toHaveBeenCalledWith("Who should I take?", [], {
      model: "opus",
      effort: null,
      fast: false,
      web_search: false,
    });
    expect(await screen.findByText("Take Chris Olave.")).toBeInTheDocument();
    // The question stays visible so the thread reads as a conversation.
    expect(screen.getByText("Who should I take?")).toBeInTheDocument();

    // A follow-up carries the exchange, so "why?" means something.
    testState.api.chat.mockResolvedValue(reply("Because WR is thin."));
    await user.type(question(), "Why?{Enter}");
    expect(await screen.findByText("Because WR is thin.")).toBeInTheDocument();
    expect(testState.api.chat).toHaveBeenLastCalledWith(
      "Why?",
      [
        { role: "you", text: "Who should I take?" },
        { role: "claude", text: "Take Chris Olave." },
      ],
      expect.objectContaining({ model: "opus" }),
    );
  });

  it("shows what the last answer cost and how big the thread is", async () => {
    const user = userEvent.setup();
    testState.api.chat.mockResolvedValue(reply("Olave.", { web_searches: 2 }));

    render(<Chat open onClose={() => undefined} />);
    expect(screen.queryByLabelText("Usage")).not.toBeInTheDocument();
    await user.type(question(), "Who?{Enter}");
    await screen.findByText("Olave.");

    expect(screen.getByLabelText("Usage")).toHaveTextContent(
      "Context 30.0k tokens · 9 s · Opus · 1 question · $0.31 · 2 web searches",
    );
  });

  it("surfaces a failure instead of dropping the question silently", async () => {
    const user = userEvent.setup();
    testState.api.chat.mockRejectedValue(new Error("Claude CLI error: not logged in"));

    render(<Chat open onClose={() => undefined} />);
    await user.type(question(), "Who?");
    await user.click(askButton());

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Claude CLI error: not logged in");
    // No "Error: " prefix leaking from String(error).
    expect(alert).not.toHaveTextContent("Error: Claude");
  });

  it("blocks a second question while one is in flight", async () => {
    const user = userEvent.setup();
    const release = pendingAnswer();

    render(<Chat open onClose={() => undefined} />);
    await user.type(question(), "Who?");
    await user.click(askButton());

    expect(await screen.findByText(/Thinking…/)).toBeInTheDocument();
    // Ask is replaced by Cancel, and Enter in the box does not send.
    expect(screen.queryByRole("button", { name: "Ask" })).not.toBeInTheDocument();
    await user.type(question(), "Another?{Enter}");

    release("Take Olave.");
    await waitFor(() => expect(screen.getByText("Take Olave.")).toBeInTheDocument());
    expect(testState.api.chat).toHaveBeenCalledTimes(1);
  });

  it("cancel frees the panel and discards the late answer", async () => {
    const user = userEvent.setup();
    const release = pendingAnswer();

    render(<Chat open onClose={() => undefined} />);
    await user.type(question(), "Who?");
    await user.click(askButton());
    await user.click(await screen.findByRole("button", { name: "Cancel" }));

    expect(screen.getByText(/Cancelled/)).toBeInTheDocument();
    expect(screen.queryByText(/Thinking…/)).not.toBeInTheDocument();
    expect(askButton()).toBeInTheDocument();

    // The model finishes after the cancel; its answer must not appear.
    await act(async () => {
      release("Too late.");
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(screen.queryByText("Too late.")).not.toBeInTheDocument();

    // And the panel is usable again immediately.
    testState.api.chat.mockResolvedValue(reply("Take Olave."));
    await user.type(question(), "Who now?");
    await user.click(askButton());
    expect(await screen.findByText("Take Olave.")).toBeInTheDocument();
  });

  it("Escape closes the panel", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();

    render(<Chat open onClose={onClose} />);
    expect(question()).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("Escape still closes the panel after focus has left it", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    testState.api.chat.mockResolvedValue(reply("Take Olave."));

    render(<Chat open onClose={onClose} />);
    // Clicking a suggestion removes that button from the DOM, so focus falls
    // to <body>; a handler scoped to the panel would never see the key.
    await user.click(screen.getByRole("button", { name: "Who should I take next?" }));
    expect(await screen.findByText("Take Olave.")).toBeInTheDocument();
    // After the answer lands the question box has focus again.
    expect(question()).toHaveFocus();

    (document.activeElement as HTMLElement | null)?.blur();
    expect(document.body).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("asks a suggested question on click", async () => {
    const user = userEvent.setup();
    testState.api.chat.mockResolvedValue(reply("Weakest at WR."));

    render(<Chat open onClose={() => undefined} />);
    await user.click(screen.getByRole("button", { name: "What position am I weakest at?" }));

    expect(testState.api.chat).toHaveBeenCalledWith(
      "What position am I weakest at?",
      [],
      expect.anything(),
    );
    expect(await screen.findByText("Weakest at WR.")).toBeInTheDocument();
  });

  it("ignores a blank question", async () => {
    const user = userEvent.setup();
    render(<Chat open onClose={() => undefined} />);

    await user.type(question(), "   ");
    expect(askButton()).toBeDisabled();
    expect(testState.api.chat).not.toHaveBeenCalled();
  });

  it("New chat starts an empty thread that sends no history", async () => {
    const user = userEvent.setup();
    testState.api.chat.mockResolvedValue(reply("Olave."));

    render(<Chat open onClose={() => undefined} />);
    expect(screen.getByRole("button", { name: "New chat" })).toBeDisabled();
    await user.type(question(), "Who?{Enter}");
    await screen.findByText("Olave.");

    await user.click(screen.getByRole("button", { name: "New chat" }));
    expect(screen.queryByText("Olave.")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Usage")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Who should I take next?" })).toBeInTheDocument();

    await user.type(question(), "Again?{Enter}");
    expect(testState.api.chat).toHaveBeenLastCalledWith("Again?", [], expect.anything());
  });

  it("Compact folds the thread into a summary that replaces it as history", async () => {
    const user = userEvent.setup();
    testState.api.chat.mockResolvedValue(reply("Olave."));
    const compact = screen.queryByRole("button", { name: "Compact" });
    expect(compact).toBeNull();

    render(<Chat open onClose={() => undefined} />);
    // Needs a real conversation first.
    expect(screen.getByRole("button", { name: "Compact" })).toBeDisabled();
    await user.type(question(), "Who?{Enter}");
    await screen.findByText("Olave.");
    expect(screen.getByRole("button", { name: "Compact" })).toBeDisabled();
    await user.type(question(), "Why?{Enter}");
    await waitFor(() => expect(screen.getAllByText("Olave.")).toHaveLength(2));
    const button = screen.getByRole("button", { name: "Compact" });
    expect(button).toBeEnabled();
    expect(button).toHaveAttribute("title", expect.stringMatching(/minute or two/));

    let release: (value: ChatReply) => void = () => undefined;
    testState.api.chatCompact.mockReturnValue(
      new Promise<ChatReply>((resolve) => {
        release = resolve;
      }),
    );
    await user.click(button);
    expect(screen.getByText(/Compacting the conversation/)).toBeInTheDocument();
    expect(testState.api.chatCompact).toHaveBeenCalledWith(
      [
        { role: "you", text: "Who?" },
        { role: "claude", text: "Olave." },
        { role: "you", text: "Why?" },
        { role: "claude", text: "Olave." },
      ],
      expect.anything(),
    );

    await act(async () => {
      release(reply("User wants Olave; WR is thin.", { context_tokens: 900 }));
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(screen.getByText("Summary so far")).toBeInTheDocument();
    expect(screen.getByText("User wants Olave; WR is thin.")).toBeInTheDocument();
    expect(screen.queryByText("Why?")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Usage")).toHaveTextContent("Context 900 tokens");

    // The summary now stands in for the earlier turns.
    testState.api.chat.mockResolvedValue(reply("Still Olave."));
    await user.type(question(), "And now?{Enter}");
    expect(testState.api.chat).toHaveBeenLastCalledWith(
      "And now?",
      [{ role: "summary", text: "User wants Olave; WR is thin." }],
      expect.anything(),
    );
  });

  it("settings reach the backend and survive a remount", async () => {
    const user = userEvent.setup();
    testState.api.chat.mockResolvedValue(reply("Olave."));

    const { unmount } = render(<Chat open onClose={() => undefined} />);
    const settings = screen.getByText("Settings").closest("details") as HTMLDetailsElement;
    expect(settings).toHaveTextContent("Opus · default effort · standard speed · web off");
    await user.selectOptions(within(settings).getByLabelText("Model"), "sonnet");
    await user.selectOptions(within(settings).getByLabelText("Thinking effort"), "low");
    await user.click(within(settings).getByLabelText(/Web search/));
    expect(settings).toHaveTextContent("Sonnet · low effort · standard speed · web on");

    await user.type(question(), "Who?{Enter}");
    await screen.findByText("Olave.");
    expect(testState.api.chat).toHaveBeenCalledWith("Who?", [], {
      model: "sonnet",
      effort: "low",
      fast: false,
      web_search: true,
    });

    unmount();
    render(<Chat open onClose={() => undefined} />);
    expect(screen.getByText("Settings").closest("details")).toHaveTextContent(
      "Sonnet · low effort · standard speed · web on",
    );
  });

  it("says once when fast mode was asked for but did not serve", async () => {
    const user = userEvent.setup();
    testState.api.chat.mockResolvedValue(
      reply("Olave.", { fast_mode: "off", fast_mode_reason: "extra_usage_disabled" }),
    );

    render(<Chat open onClose={() => undefined} />);
    await user.click(screen.getByLabelText(/Fast mode/));
    await user.type(question(), "Who?{Enter}");
    await screen.findByText("Olave.");
    expect(
      screen.getByText("Fast mode unavailable (extra_usage_disabled) — answered at standard speed."),
    ).toBeInTheDocument();

    await user.type(question(), "Why?{Enter}");
    await waitFor(() => expect(screen.getAllByText("Olave.")).toHaveLength(2));
    expect(screen.getAllByText(/Fast mode unavailable/)).toHaveLength(1);
  });
});
