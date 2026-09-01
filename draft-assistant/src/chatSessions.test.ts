import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ThreadEntry } from "./chat-types";
import {
  deleteSession,
  describeSession,
  listSessions,
  loadSession,
  newSessionId,
  saveSession,
  sessionTitle,
  type SavedChat,
} from "./chatSessions";

function chat(overrides: Partial<SavedChat>): SavedChat {
  const entries: ThreadEntry[] = [
    { id: 1, kind: "me", lines: ["Who should I take?"] },
    { id: 2, kind: "claude", lines: ["The running back."] },
  ];
  return {
    id: "s1",
    title: "Who should I take?",
    startedAt: 1_700_000_000_000,
    updatedAt: 1_700_000_060_000,
    entries,
    history: [{ role: "user", content: "Who should I take?" }],
    questions: 1,
    costUsd: 0.25,
    ...overrides,
  };
}

beforeEach(() => {
  localStorage.clear();
});

describe("saved chats", () => {
  it("round-trips a conversation, turns and all", () => {
    saveSession("draft", chat({}));
    const stored = loadSession("draft", "s1");
    expect(stored?.entries).toHaveLength(2);
    expect(stored?.history).toEqual([{ role: "user", content: "Who should I take?" }]);
    expect(stored?.costUsd).toBe(0.25);
  });

  it("keeps each screen's chats apart", () => {
    saveSession("draft", chat({}));
    expect(listSessions("season")).toEqual([]);
    expect(listSessions("draft")).toHaveLength(1);
  });

  it("saving the same id replaces it rather than listing it twice", () => {
    saveSession("draft", chat({ questions: 1 }));
    saveSession("draft", chat({ questions: 4, updatedAt: 1_700_000_120_000 }));
    const listed = listSessions("draft");
    expect(listed).toHaveLength(1);
    expect(listed[0].questions).toBe(4);
  });

  it("lists newest activity first", () => {
    saveSession("draft", chat({ id: "old", updatedAt: 1 }));
    saveSession("draft", chat({ id: "new", updatedAt: 2 }));
    expect(listSessions("draft").map((s) => s.id)).toEqual(["new", "old"]);
  });

  it("forgets one without touching the others", () => {
    saveSession("draft", chat({ id: "a" }));
    saveSession("draft", chat({ id: "b" }));
    deleteSession("draft", "a");
    expect(listSessions("draft").map((s) => s.id)).toEqual(["b"]);
    expect(loadSession("draft", "a")).toBeNull();
  });

  it("keeps only the twenty most recent", () => {
    for (let i = 0; i < 25; i++) saveSession("draft", chat({ id: `s${i}`, updatedAt: i }));
    const listed = listSessions("draft");
    expect(listed).toHaveLength(20);
    expect(listed[listed.length - 1].id).toBe("s5");
  });

  it("drops stored text that is not a conversation instead of throwing", () => {
    localStorage.setItem("da.chat.sessions.draft", '[{"id":"x"},"nonsense"]');
    expect(listSessions("draft")).toEqual([]);
    localStorage.setItem("da.chat.sessions.draft", "not json at all");
    expect(listSessions("draft")).toEqual([]);
  });

  it("survives storage refusing to answer, as a private window does", () => {
    vi.stubGlobal("localStorage", {
      getItem: () => {
        throw new Error("storage is not available");
      },
      setItem: () => {
        throw new Error("storage is not available");
      },
    });
    expect(listSessions("draft")).toEqual([]);
    expect(loadSession("draft", "s1")).toBeNull();
    expect(() => saveSession("draft", chat({}))).not.toThrow();
    expect(() => deleteSession("draft", "s1")).not.toThrow();
    vi.unstubAllGlobals();
  });
});

describe("how a chat is named and listed", () => {
  it("titles a conversation with its first question, clipped", () => {
    expect(sessionTitle([{ id: 1, kind: "me", lines: ["Who   should\nI take?"] }])).toBe(
      "Who should I take?",
    );
    const long = "x".repeat(80);
    expect(sessionTitle([{ id: 1, kind: "me", lines: [long] }])).toHaveLength(52);
    expect(sessionTitle([{ id: 1, kind: "claude", lines: ["hi"] }])).toBe("New chat");
  });

  it("describes a chat by time, question, count and cost", () => {
    const line = describeSession({
      id: "s1",
      title: "Who should I take?",
      startedAt: Date.UTC(2026, 7, 30, 16, 41),
      updatedAt: 0,
      questions: 1,
      costUsd: 0.666,
    });
    // Eastern time, like every other timestamp in the app — 16:41 UTC in
    // August is 12:41 in New York, whatever this machine's clock is set to.
    expect(line).toBe("Aug 30, 12:41 PM · Who should I take? · 1 question · $0.67");
  });

  it("shows a conversation worth less than a cent as more than nothing", () => {
    const line = describeSession({
      id: "s1",
      title: "Quick one",
      startedAt: Date.UTC(2026, 7, 30, 16, 41),
      updatedAt: 0,
      questions: 1,
      costUsd: 0.004,
    });
    expect(line).toContain("· $0.004");
  });

  it("gives every new chat its own id", () => {
    expect(new Set([newSessionId(), newSessionId(), newSessionId()]).size).toBeGreaterThan(1);
  });
});
