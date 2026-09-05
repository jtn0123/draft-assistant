// The shared thread's own controls, rendered on their own rather than through
// the whole Ask Claude panel.

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
import type { SharedChatEntry, SharedChatThread } from "../types";

const mocks = vi.hoisted(() => ({
  sharedChatGet: vi.fn(),
  sharedChatSend: vi.fn(),
  sharedChatReset: vi.fn(),
  onSharedChat: vi.fn(),
}));

vi.mock("../api", () => ({ api: mocks }));

import { SharedChat } from "./SharedChat";

function entry(overrides: Partial<SharedChatEntry> = {}): SharedChatEntry {
  return {
    id: "e1",
    at_ms: 1,
    device: { name: "Rob's iPhone", kind: "phone" },
    role: "user",
    text: "who should I take?",
    cost_usd: null,
    error: null,
    ...overrides,
  };
}

function thread(overrides: Partial<SharedChatThread> = {}): SharedChatThread {
  return { league_id: "1", screen: "draft", busy: false, entries: [], ...overrides };
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.sharedChatGet.mockResolvedValue(thread({ entries: [entry()] }));
  mocks.sharedChatReset.mockResolvedValue(undefined);
  mocks.onSharedChat.mockResolvedValue(() => undefined);
});

/** The thread keeps two hundred entries, there is one per screen per league,
 *  and a phone has no saved-conversations picker: without this there was no
 *  way at all to start a shared conversation over. */
it("empties the thread for everyone once the asking is confirmed", async () => {
  const user = userEvent.setup();
  render(<SharedChat screen="draft" compact={false} />);
  const button = await screen.findByRole("button", { name: "New thread" });

  // One click asks rather than empties: it is everyone's thread.
  await user.click(button);
  expect(mocks.sharedChatReset).not.toHaveBeenCalled();
  const confirm = await screen.findByRole("button", { name: "Empty it for everyone?" });

  await user.click(confirm);
  await waitFor(() => expect(mocks.sharedChatReset).toHaveBeenCalledWith("draft"));
  // And it goes back to asking rather than staying armed.
  expect(await screen.findByRole("button", { name: "New thread" })).toBeTruthy();
});

it("says so when the host refuses to empty the thread", async () => {
  const user = userEvent.setup();
  mocks.sharedChatReset.mockRejectedValue(new Error("the host said no"));
  render(<SharedChat screen="draft" compact={false} />);
  await user.click(await screen.findByRole("button", { name: "New thread" }));
  await user.click(await screen.findByRole("button", { name: "Empty it for everyone?" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("the host said no");
});

it("has nothing to empty on a thread nobody has asked on yet", async () => {
  mocks.sharedChatGet.mockResolvedValue(thread());
  render(<SharedChat screen="draft" compact={false} />);
  await waitFor(() => expect(screen.getByRole("button", { name: "New thread" })).toBeDisabled());
});

/** A question is already being answered on the host's budget; emptying the
 *  thread under it would leave the answer arriving into nothing. */
it("cannot be emptied while an answer is on its way", async () => {
  mocks.sharedChatGet.mockResolvedValue(thread({ busy: true, entries: [entry()] }));
  render(<SharedChat screen="draft" compact={false} />);
  await waitFor(() => expect(screen.getByRole("button", { name: "New thread" })).toBeDisabled());
});
