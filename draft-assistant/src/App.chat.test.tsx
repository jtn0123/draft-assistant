// The Ask AI panel inside the shell: closed with the header button and
// opened again while an answer is still on its way.

import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./api", async () => ({ api: (await import("./test/appHarness")).harness().api }));
vi.mock("./chatCancel", () => ({ cancelClaude: vi.fn(() => Promise.resolve(true)) }));

import "./test/warmScreens";
import App from "./App";
import type { ChatReply } from "./chat-types";
import { resetPendingTurns } from "./chatPending";
import { listSessions } from "./chatSessions";
import { resetPrefs } from "./prefs";
import { resetThemePreference } from "./theme";
import { settle } from "./test/settle";
import { draftFixture, fakeStorage, harness, restoringConfig } from "./test/appHarness";

const h = harness();

async function loaded() {
  const view = draftFixture();
  h.api.getConfig.mockResolvedValue(restoringConfig(view));
  h.api.addLeague.mockResolvedValue(view);
  render(<App />);
  await screen.findByRole("heading", { name: view.league.name });
  await settle(() => {});
  return view;
}

function reply(overrides: Partial<ChatReply>): ChatReply {
  return {
    text: "",
    thinking: null,
    model: "Opus 5",
    refused: false,
    truncated: false,
    cancelled: false,
    input_tokens: 0,
    output_tokens: 0,
    provider: "api",
    cost_usd: 0,
    screen_spend_usd: 0,
    ...overrides,
  };
}

beforeEach(() => {
  h.reset();
  resetPendingTurns();
  fakeStorage({ "da.screen": "draft", "da.askButton": "on" });
  resetPrefs();
  resetThemePreference();
  Element.prototype.scrollTo = vi.fn();
  h.api.chatSettings.mockResolvedValue({
    has_key: true,
    key_hint: "····abcd",
    cli_available: false,
    provider: "api",
    key_store: "keychain",
    budget_usd: 5,
    spend_usd: {},
    models: ["Opus 5", "Fable 5.1"],
    efforts: { "Opus 5": ["Off", "High"], "Fable 5.1": ["Low", "High"] },
    notes: {},
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

const askButton = () => screen.getByRole("button", { name: "Ask AI" });

describe("the chat panel in the shell", () => {
  it("keeps a question, its thinking state and its spend across a close and reopen", async () => {
    const view = await loaded();
    let resolve!: (reply: ChatReply) => void;
    h.api.askClaude.mockReturnValue(
      new Promise<ChatReply>((r) => {
        resolve = r;
      }),
    );

    await settle(() => askButton().click());
    const input = await screen.findByRole("textbox", { name: "Ask AI" });
    await userEvent.type(input, "Who should I take?{Enter}");
    expect(screen.getByText(/Thinking it through/)).toBeInTheDocument();

    // Closing the panel unmounts it. The question, and the money the answer
    // was about to cost, used to go with it.
    await settle(() => screen.getByRole("button", { name: "Close" }).click());
    expect(screen.queryByRole("textbox", { name: "Ask AI" })).not.toBeInTheDocument();

    await settle(() => askButton().click());
    expect(await screen.findByText("Who should I take?")).toBeInTheDocument();
    expect(screen.getByText(/Thinking it through/)).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Ask AI" })).toBeDisabled();

    await act(async () => {
      resolve(reply({ text: "The RB.", cost_usd: 0.05, screen_spend_usd: 0.05 }));
      await Promise.resolve();
    });
    expect(await screen.findByText("The RB.")).toBeInTheDocument();
    expect(screen.getByText(/\$0\.05 estimated cost/)).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Ask AI" })).toBeEnabled();
    // One call: the reopened panel picked the answer up rather than asking again.
    expect(h.api.askClaude).toHaveBeenCalledTimes(1);
    expect(listSessions(`draft.${view.league.league_id}`)[0]).toMatchObject({
      questions: 1,
      costUsd: 0.05,
    });
  });

  it("files the answer into the conversation when the panel is left closed", async () => {
    const view = await loaded();
    let resolve!: (reply: ChatReply) => void;
    h.api.askClaude.mockReturnValue(
      new Promise<ChatReply>((r) => {
        resolve = r;
      }),
    );
    await settle(() => askButton().click());
    await userEvent.type(
      await screen.findByRole("textbox", { name: "Ask AI" }),
      "Who should I take?{Enter}",
    );
    await settle(() => screen.getByRole("button", { name: "Close" }).click());

    await act(async () => {
      resolve(reply({ text: "The RB.", cost_usd: 0.05, screen_spend_usd: 0.05 }));
      await Promise.resolve();
      await Promise.resolve();
    });
    const scope = `draft.${view.league.league_id}`;
    expect(listSessions(scope)).toHaveLength(1);
    expect(listSessions(scope)[0]).toMatchObject({
      title: "Who should I take?",
      questions: 1,
      costUsd: 0.05,
    });

    await settle(() => askButton().click());
    expect(await screen.findByText("The RB.")).toBeInTheDocument();
    expect(screen.getByText(/\$0\.05 estimated cost/)).toBeInTheDocument();
  });
});
