import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const testState = vi.hoisted(() => ({
  api: { chat: vi.fn() },
}));

vi.mock("../api", () => ({ api: testState.api }));

import { Chat } from "./Chat";

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

    expect(await screen.findByRole("alert")).toHaveTextContent("not logged in");
  });

  it("blocks a second question while one is in flight", async () => {
    const user = userEvent.setup();
    let release: (value: string) => void = () => undefined;
    testState.api.chat.mockReturnValue(
      new Promise<string>((resolve) => {
        release = resolve;
      }),
    );

    render(<Chat open onClose={() => undefined} />);
    await user.type(screen.getByLabelText("Your question"), "Who?");
    await user.click(screen.getByRole("button", { name: "Ask" }));

    expect(await screen.findByText(/Thinking/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Asking…" })).toBeDisabled();

    release("Take Olave.");
    await waitFor(() => expect(screen.getByText("Take Olave.")).toBeInTheDocument());
    expect(testState.api.chat).toHaveBeenCalledTimes(1);
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
