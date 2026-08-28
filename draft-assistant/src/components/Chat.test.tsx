import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const testState = vi.hoisted(() => ({
  api: { chat: vi.fn() },
}));

vi.mock("../api", () => ({ api: testState.api }));

import { Chat } from "./Chat";

/** A chat call that stays pending until the test releases it. */
function pendingAnswer(): (value: string) => void {
  let release: (value: string) => void = () => undefined;
  testState.api.chat.mockReturnValue(
    new Promise<string>((resolve) => {
      release = resolve;
    }),
  );
  return (value) => release(value);
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("Chat panel", () => {
  it("renders nothing while closed", () => {
    const { container } = render(<Chat open={false} onClose={() => undefined} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("sends a question and shows the answer", async () => {
    const user = userEvent.setup();
    testState.api.chat.mockResolvedValue("Take Chris Olave.");

    render(<Chat open onClose={() => undefined} />);
    await user.type(screen.getByLabelText("Your question"), "Who should I take?");
    await user.click(screen.getByRole("button", { name: "Ask" }));

    expect(testState.api.chat).toHaveBeenCalledWith("Who should I take?");
    expect(await screen.findByText("Take Chris Olave.")).toBeInTheDocument();
    // The question stays visible so the thread reads as a conversation.
    expect(screen.getByText("Who should I take?")).toBeInTheDocument();
  });

  it("surfaces a failure instead of dropping the question silently", async () => {
    const user = userEvent.setup();
    testState.api.chat.mockRejectedValue(new Error("Claude CLI error: not logged in"));

    render(<Chat open onClose={() => undefined} />);
    await user.type(screen.getByLabelText("Your question"), "Who?");
    await user.click(screen.getByRole("button", { name: "Ask" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Claude CLI error: not logged in");
    // No "Error: " prefix leaking from String(error).
    expect(alert).not.toHaveTextContent("Error: Claude");
  });

  it("blocks a second question while one is in flight", async () => {
    const user = userEvent.setup();
    const release = pendingAnswer();

    render(<Chat open onClose={() => undefined} />);
    await user.type(screen.getByLabelText("Your question"), "Who?");
    await user.click(screen.getByRole("button", { name: "Ask" }));

    expect(await screen.findByText(/Thinking/)).toBeInTheDocument();
    // Ask is replaced by Cancel, and Enter in the box does not send.
    expect(screen.queryByRole("button", { name: "Ask" })).not.toBeInTheDocument();
    await user.type(screen.getByLabelText("Your question"), "Another?{Enter}");

    release("Take Olave.");
    await waitFor(() => expect(screen.getByText("Take Olave.")).toBeInTheDocument());
    expect(testState.api.chat).toHaveBeenCalledTimes(1);
  });

  it("cancel frees the panel and discards the late answer", async () => {
    const user = userEvent.setup();
    const release = pendingAnswer();

    render(<Chat open onClose={() => undefined} />);
    await user.type(screen.getByLabelText("Your question"), "Who?");
    await user.click(screen.getByRole("button", { name: "Ask" }));
    await user.click(await screen.findByRole("button", { name: "Cancel" }));

    expect(screen.getByText(/Cancelled/)).toBeInTheDocument();
    expect(screen.queryByText(/Thinking/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Ask" })).toBeInTheDocument();

    // The model finishes after the cancel; its answer must not appear.
    await act(async () => {
      release("Too late.");
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(screen.queryByText("Too late.")).not.toBeInTheDocument();

    // And the panel is usable again immediately.
    testState.api.chat.mockResolvedValue("Take Olave.");
    await user.type(screen.getByLabelText("Your question"), "Who now?");
    await user.click(screen.getByRole("button", { name: "Ask" }));
    expect(await screen.findByText("Take Olave.")).toBeInTheDocument();
  });

  it("Escape closes the panel", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();

    render(<Chat open onClose={onClose} />);
    expect(screen.getByLabelText("Your question")).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("Escape still closes the panel after focus has left it", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    testState.api.chat.mockResolvedValue("Take Olave.");

    render(<Chat open onClose={onClose} />);
    // Clicking a suggestion removes that button from the DOM, so focus falls
    // to <body>; a handler scoped to the panel would never see the key.
    await user.click(screen.getByRole("button", { name: "Who should I take next?" }));
    expect(await screen.findByText("Take Olave.")).toBeInTheDocument();
    // After the answer lands the question box has focus again.
    expect(screen.getByLabelText("Your question")).toHaveFocus();

    (document.activeElement as HTMLElement | null)?.blur();
    expect(document.body).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("asks a suggested question on click", async () => {
    const user = userEvent.setup();
    testState.api.chat.mockResolvedValue("Weakest at WR.");

    render(<Chat open onClose={() => undefined} />);
    await user.click(screen.getByRole("button", { name: "What position am I weakest at?" }));

    expect(testState.api.chat).toHaveBeenCalledWith("What position am I weakest at?");
    expect(await screen.findByText("Weakest at WR.")).toBeInTheDocument();
  });

  it("ignores a blank question", async () => {
    const user = userEvent.setup();
    render(<Chat open onClose={() => undefined} />);

    await user.type(screen.getByLabelText("Your question"), "   ");
    expect(screen.getByRole("button", { name: "Ask" })).toBeDisabled();
    expect(testState.api.chat).not.toHaveBeenCalled();
  });
});
