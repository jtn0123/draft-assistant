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
