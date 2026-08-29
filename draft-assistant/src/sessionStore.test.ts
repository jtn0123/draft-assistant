import { beforeEach, describe, expect, it } from "vitest";
import { browserListSessions, browserLoadSession, browserSaveSession } from "./sessionStore";
import type { ChatSession } from "./types";

function session(id: string, draftId: string, updatedAt: number): ChatSession {
  return {
    id,
    draft_id: draftId,
    league_name: "L",
    started_at: updatedAt - 10,
    updated_at: updatedAt,
    title: `Q ${id}`,
    turns: [{ role: "you", text: `Q ${id}`, as_of_pick: null }],
    questions: 1,
    cost_usd: 0.1,
  };
}

beforeEach(() => window.localStorage.clear());

describe("browser session store", () => {
  it("round-trips a session and lists per draft, newest activity first", () => {
    const where = browserSaveSession(session("a", "d1", 100));
    expect(where).toBe("localStorage:draft-assistant.chat-sessions.d1/a");
    browserSaveSession(session("b", "d1", 300));
    browserSaveSession(session("c", "d1", 200));
    browserSaveSession(session("z", "d2", 999));
    expect(browserListSessions("d1").map((s) => s.id)).toEqual(["b", "c", "a"]);
    expect(browserListSessions("d2")).toHaveLength(1);
    expect(browserListSessions("d3")).toEqual([]);
    expect(browserLoadSession("d1", "c")).toEqual(session("c", "d1", 200));
  });

  it("saving again replaces the entry, and a missing or broken store is empty", () => {
    browserSaveSession(session("a", "d1", 100));
    browserSaveSession({ ...session("a", "d1", 500), questions: 3 });
    expect(browserListSessions("d1")).toHaveLength(1);
    expect(browserListSessions("d1")[0].questions).toBe(3);
    expect(() => browserLoadSession("d1", "nope")).toThrow("chat session nope could not be read");
    window.localStorage.setItem("draft-assistant.chat-sessions.d1", "{broken");
    expect(browserListSessions("d1")).toEqual([]);
  });
});
