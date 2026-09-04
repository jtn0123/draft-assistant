// The pinned "Shared with devices" thread inside the Ask Claude panel.

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatSettings } from "../chat-types";
import type { SharedChatEntry, SharedChatThread } from "../types";

const mocks = vi.hoisted(() => ({
  chatSettings: vi.fn(),
  chatSuggestions: vi.fn(),
  setChatProvider: vi.fn(),
  setChatBudget: vi.fn(),
  askClaude: vi.fn(),
  sharedChatGet: vi.fn(),
  sharedChatSend: vi.fn(),
  onSharedChat: vi.fn(),
}));

vi.mock("../api", () => ({ api: mocks }));

import { Chat } from "./Chat";

function settings(): ChatSettings {
  return {
    has_key: true,
    key_hint: null,
    cli_available: false,
    provider: "api",
    key_store: "keychain",
    budget_usd: 5,
    spend_usd: {},
    models: ["Opus 5"],
    efforts: { "Opus 5": ["High"] },
    notes: {},
  };
}

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

/** The handler the panel registered for `shared-chat`. */
let pushThread: ((next: SharedChatThread) => void) | null = null;

beforeEach(() => {
  // jsdom has no scrollTo; the local thread scrolls itself as it grows.
  Element.prototype.scrollTo = vi.fn();
  for (const mock of Object.values(mocks)) mock.mockReset();
  pushThread = null;
  mocks.chatSettings.mockResolvedValue(settings());
  mocks.chatSuggestions.mockResolvedValue([]);
  mocks.sharedChatGet.mockResolvedValue(thread());
  mocks.sharedChatSend.mockResolvedValue(undefined);
  mocks.onSharedChat.mockImplementation((handler: (next: SharedChatThread) => void) => {
    pushThread = handler;
    return Promise.resolve(() => undefined);
  });
});

const panel = (sharedOnly = false) =>
  render(
    <Chat
      screen="draft"
      leagueId="1"
      contextNote="Sees this draft"
      sharedOnly={sharedOnly}
      onClose={() => undefined}
    />,
  );

/** Pick the pinned thread out of the saved-chats box. */
async function openShared() {
  await userEvent.selectOptions(await screen.findByLabelText("Saved chats"), "shared");
}

describe("picking the shared thread", () => {
  it("is pinned in the picker and opens on the host's thread", async () => {
    panel();
    expect(await screen.findByRole("option", { name: "Shared with devices" })).toBeInTheDocument();
    await openShared();
    expect(mocks.sharedChatGet).toHaveBeenCalledWith("draft");
    expect(
      await screen.findByText(/Everyone paired with this app reads this thread/),
    ).toBeInTheDocument();
  });

  it("leaves the local conversations alone", async () => {
    panel();
    await openShared();
    // The local composer, its model picker and its budget belong to the saved
    // chats; only the picker itself stays put.
    expect(screen.queryByLabelText("Ask Claude")).not.toBeInTheDocument();
    await userEvent.selectOptions(screen.getByLabelText("Saved chats"), [
      screen.getByRole("option", { name: /nothing asked yet/ }),
    ]);
    expect(await screen.findByLabelText("Ask Claude")).toBeInTheDocument();
  });
});

describe("the thread", () => {
  it("attributes every entry to the device that wrote it, with what it cost", async () => {
    mocks.sharedChatGet.mockResolvedValue(
      thread({
        entries: [
          entry(),
          entry({
            id: "e2",
            role: "assistant",
            text: "Take the running back.",
            cost_usd: 0.42,
          }),
          entry({ id: "e3", device: { name: "Justin's Mac", kind: "desktop" }, text: "thanks" }),
        ],
      }),
    );
    panel();
    await openShared();

    expect(await screen.findByText("Take the running back.")).toBeInTheDocument();
    // The answer carries the device that asked, not the host.
    expect(screen.getAllByText("Rob's iPhone")).toHaveLength(2);
    expect(screen.getByText("Justin's Mac")).toBeInTheDocument();
    expect(screen.getByText("$0.42")).toBeInTheDocument();
  });

  it("follows the host's pushes", async () => {
    panel();
    await openShared();
    await waitFor(() => expect(pushThread).not.toBeNull());
    pushThread?.(thread({ entries: [entry({ text: "is Bijan there?" })] }));
    expect(await screen.findByText("is Bijan there?")).toBeInTheDocument();
  });

  it("says who is being answered and holds the composer while it is", async () => {
    mocks.sharedChatGet.mockResolvedValue(thread({ busy: true, entries: [entry()] }));
    panel();
    await openShared();

    expect(await screen.findByText(/Answering Rob's iPhone…/)).toBeInTheDocument();
    expect(screen.getByLabelText("Ask on the shared thread")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
  });
});

describe("asking on it", () => {
  it("sends to the host rather than answering locally", async () => {
    panel();
    await openShared();
    const box = await screen.findByLabelText("Ask on the shared thread");
    await userEvent.type(box, "who is left at RB?{Enter}");

    expect(mocks.sharedChatSend).toHaveBeenCalledWith("draft", "who is left at RB?");
    expect(mocks.askClaude).not.toHaveBeenCalled();
  });

  it("keeps the question and says why when the host refuses it", async () => {
    mocks.sharedChatSend.mockRejectedValue(new Error("Someone else is asking"));
    panel();
    await openShared();
    const box = await screen.findByLabelText("Ask on the shared thread");
    await userEvent.type(box, "who is left?{Enter}");

    expect(await screen.findByRole("alert")).toHaveTextContent("Someone else is asking");
    await waitFor(() => expect(box).toHaveValue("who is left?"));
  });
});

describe("follower mode", () => {
  it("offers the shared thread and nothing else", async () => {
    panel(true);
    expect(
      await screen.findByText(/Everyone paired with this app reads this thread/),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("Saved chats")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Ask Claude")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Ask on the shared thread")).toBeEnabled();
  });
});
