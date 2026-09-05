// The key form's one failure path: what it says, and how a screen reader
// finds out it said it.

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ setApiKey: vi.fn() }));
vi.mock("../api", () => ({ api: mocks }));

import { ChatKeyForm } from "./ChatKeyForm";

beforeEach(() => {
  vi.clearAllMocks();
});

describe("saving a key that the backend refuses", () => {
  it("shows the message without the Error prefix, and announces it", async () => {
    // `String(e)` on an Error prints "Error: that key is not valid", and the
    // panel showed that verbatim.
    mocks.setApiKey.mockRejectedValue(new Error("that key is not valid"));
    render(<ChatKeyForm hint={null} store="keychain" onSaved={() => undefined} />);

    await userEvent.type(screen.getByLabelText("Anthropic API key"), "sk-ant-x");
    await userEvent.click(screen.getByRole("button", { name: "Save key" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("that key is not valid");
    expect(alert.textContent).not.toMatch(/^Error:/);
  });

  it("says nothing and hands the key on when it is accepted", async () => {
    mocks.setApiKey.mockResolvedValue(undefined);
    const onSaved = vi.fn();
    render(<ChatKeyForm hint="sk-ant-…9f2" store="file" onSaved={onSaved} />);

    await userEvent.type(screen.getByLabelText("Anthropic API key"), "sk-ant-y");
    await userEvent.click(screen.getByRole("button", { name: "Save key" }));

    await waitFor(() => expect(onSaved).toHaveBeenCalled());
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
