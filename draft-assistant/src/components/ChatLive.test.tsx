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

function reply(answer: string, over: Partial<ChatUsage> = {}, pick = 12): ChatReply {
  return { answer, usage: usage(over), as_of: { pick, seq: 1 } };
}

const question = () => screen.getByLabelText("Your question");
const askButton = () => screen.getByRole("button", { name: "Ask" });

beforeEach(() => {
  vi.clearAllMocks();
  window.localStorage.clear();
});

describe("Chat panel — streaming, markdown, as-of, budget, auto-ask", () => {
  it("shows the answer as it streams, then the whole answer", async () => {
    const user = userEvent.setup();
    let stream: (text: string) => void = () => undefined;
    let release: (value: ChatReply) => void = () => undefined;
    testState.api.chat.mockImplementation(
      (_q: string, _h: unknown, _o: unknown, onText: (text: string) => void) => {
        stream = onText;
        return new Promise<ChatReply>((resolve) => {
          release = resolve;
        });
      },
    );

    render(<Chat open onClose={() => undefined} />);
    await user.type(question(), "Who?{Enter}");
    expect(await screen.findByText(/Thinking…/)).toBeInTheDocument();

    act(() => stream("Take "));
    act(() => stream("**Gibbs**"));
    // The placeholder gives way to the words so far, rendered as they land.
    expect(screen.queryByText(/Thinking…/)).not.toBeInTheDocument();
    expect(document.querySelector(".chat-turn.streaming")).toHaveTextContent("Take Gibbs");
    expect(document.querySelector(".chat-turn.streaming strong")).toHaveTextContent("Gibbs");

    await act(async () => {
      release(reply("Take **Gibbs** at 2."));
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(document.querySelector(".chat-turn.streaming")).toBeNull();
    const answers = document.querySelectorAll(".chat-turn.claude");
    expect(answers).toHaveLength(1);
    expect(answers[0]).toHaveTextContent("Take Gibbs at 2.");
    expect(answers[0].querySelector("strong")).toHaveTextContent("Gibbs");
  });

  it("renders an answer's markdown as lists and emphasis, not asterisks", async () => {
    const user = userEvent.setup();
    testState.api.chat.mockResolvedValue(reply("Plan:\n- **Gibbs** at 2\n- WR at 27"));

    render(<Chat open onClose={() => undefined} />);
    await user.type(question(), "Plan?{Enter}");
    const items = await screen.findAllByRole("listitem");
    expect(items.map((li) => li.textContent)).toEqual(["Gibbs at 2", "WR at 27"]);
    expect(items[0].querySelector("strong")).toHaveTextContent("Gibbs");
    expect(document.body.textContent).not.toContain("**");
  });

  it("stamps each answer with the pick it saw and says when picks have passed", async () => {
    const user = userEvent.setup();
    testState.api.chat.mockResolvedValue(reply("Olave.", {}, 57));

    const { rerender } = render(<Chat open onClose={() => undefined} currentPick={57} />);
    await user.type(question(), "Who?{Enter}");
    await screen.findByText("Olave.");
    expect(screen.getByText("as of pick 57")).toBeInTheDocument();
    expect(screen.getByText("as of pick 57")).not.toHaveClass("stale");

    rerender(<Chat open onClose={() => undefined} currentPick={59} />);
    const stamp = screen.getByText(/as of pick 57/);
    expect(stamp).toHaveTextContent("as of pick 57 · 2 picks since");
    expect(stamp).toHaveClass("stale");
  });

  it("stops asking once the session budget is spent", async () => {
    const user = userEvent.setup();
    testState.api.chat.mockResolvedValue(reply("Olave.", { cost_usd: 1.5 }));

    render(<Chat open onClose={() => undefined} />);
    const settings = screen.getByText("Settings").closest("details") as HTMLDetailsElement;
    const budget = within(settings).getByLabelText("Session budget");
    await user.clear(budget);
    await user.type(budget, "2");

    await user.type(question(), "Who?{Enter}");
    await screen.findByText("Olave.");
    expect(screen.queryByRole("status")).not.toBeInTheDocument();

    await user.type(question(), "Why?{Enter}");
    await waitFor(() => expect(screen.getAllByText("Olave.")).toHaveLength(2));
    // $3.00 spent against a $2 budget: the panel says so and Ask is off.
    expect(screen.getByRole("status")).toHaveTextContent(
      "Session budget of $2.00 reached ($3.00 spent)",
    );
    await user.type(question(), "And?{Enter}");
    expect(testState.api.chat).toHaveBeenCalledTimes(2);
    expect(askButton()).toBeDisabled();

    // A new chat resets the spend.
    await user.click(screen.getByRole("button", { name: "New chat" }));
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("asks by itself when the pick comes up, once per pick, and opens the panel", async () => {
    const user = userEvent.setup();
    testState.api.chat.mockResolvedValue(reply("Take Gibbs.", {}, 2));
    const onAutoAsk = vi.fn();

    const { rerender } = render(
      <Chat open onClose={() => undefined} currentPick={1} onClock={false} onAutoAsk={onAutoAsk} />,
    );
    await user.click(screen.getByLabelText("Ask when I'm on the clock"));
    expect(testState.api.chat).not.toHaveBeenCalled();

    rerender(
      <Chat open onClose={() => undefined} currentPick={2} onClock onAutoAsk={onAutoAsk} />,
    );
    await screen.findByText("Take Gibbs.");
    expect(onAutoAsk).toHaveBeenCalledTimes(1);
    expect(testState.api.chat).toHaveBeenCalledWith(
      "Who should I take next?",
      [],
      expect.anything(),
      expect.any(Function),
    );
    expect(screen.getByText(/Asked automatically/)).toBeInTheDocument();

    // Still on the same pick: no second question.
    rerender(
      <Chat open onClose={() => undefined} currentPick={2} onClock onAutoAsk={onAutoAsk} />,
    );
    expect(testState.api.chat).toHaveBeenCalledTimes(1);

    // Next turn: asked again, with the earlier exchange as history.
    rerender(
      <Chat open onClose={() => undefined} currentPick={27} onClock onAutoAsk={onAutoAsk} />,
    );
    await waitFor(() => expect(testState.api.chat).toHaveBeenCalledTimes(2));
    expect(testState.api.chat).toHaveBeenLastCalledWith(
      "Who should I take next?",
      [
        { role: "you", text: "Who should I take next?" },
        { role: "claude", text: "Take Gibbs." },
      ],
      expect.anything(),
      expect.any(Function),
    );
  });

  it("does not ask by itself while the setting is off", () => {
    render(<Chat open onClose={() => undefined} currentPick={2} onClock />);
    expect(testState.api.chat).not.toHaveBeenCalled();
  });
});
