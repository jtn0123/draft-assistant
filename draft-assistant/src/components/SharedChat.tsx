// The shared thread: one conversation per screen per league that every paired
// device can read and add to.
//
// Nothing here is stored locally. The host owns the thread — it appends the
// question at once, answers it on its own budget, and pushes the whole thread
// back down the `shared-chat` event — so this component holds exactly one
// piece of state, the thread as last received, and sends into it.

import { useEffect, useState } from "react";
import { api } from "../api";
import { formatUsd } from "../chatCost";
import { Markdown } from "./Markdown";
import type { SharedChatEntry, SharedChatThread } from "../types";

import "../companion.css";

const EMPTY: SharedChatThread = { league_id: "", screen: "", busy: false, entries: [] };

/** Who asked, whether they are still waiting, and what the answer cost. */
function Entry({ entry }: { entry: SharedChatEntry }) {
  const kind = entry.error !== null ? "error" : entry.role === "user" ? "me" : "claude";
  return (
    <div className={`msg is-${kind}`}>
      <span className="shared-byline">
        <span className={`device-glyph is-${entry.device.kind}`} aria-hidden="true" />
        {entry.device.name}
        {entry.cost_usd !== null && (
          <span className="shared-cost">{formatUsd(entry.cost_usd)}</span>
        )}
      </span>
      {entry.error !== null ? (
        <span className="msg-line">{entry.error}</span>
      ) : entry.role === "assistant" ? (
        <Markdown text={entry.text} />
      ) : (
        <span className="msg-line">{entry.text}</span>
      )}
    </div>
  );
}

/** The device the host is currently answering — the last one to ask. */
function waitingOn(thread: SharedChatThread): string {
  const asked = [...thread.entries].reverse().find((e) => e.role === "user");
  return asked?.device.name ?? "a device";
}

export function SharedChat({ screen, compact }: { screen: string; compact: boolean }) {
  const [thread, setThread] = useState<SharedChatThread>({ ...EMPTY, screen });
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    api
      .sharedChatGet(screen)
      .then((next) => {
        if (!cancelled) setThread(next);
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [screen]);

  // Every device's questions and the host's answers arrive the same way, so
  // there is no local echo to reconcile: what is on screen is what the host
  // says the thread is.
  useEffect(() => {
    let live = true;
    const pending = api.onSharedChat((next) => {
      if (live && next.screen === screen) setThread(next);
    });
    return () => {
      live = false;
      void pending.then((off) => off());
    };
  }, [screen]);

  const send = async () => {
    const text = draft.trim();
    if (text === "" || thread.busy) return;
    setDraft("");
    setError(null);
    try {
      await api.sharedChatSend(screen, text);
    } catch (e) {
      setError(String(e));
      setDraft(text);
    }
  };

  return (
    <>
      <div className={compact ? "chat-thread is-compact" : "chat-thread"}>
        {thread.entries.length === 0 ? (
          <div className="shared-empty">
            <span className="chat-empty-title">Shared with devices</span>
            <span>
              Everyone paired with this app reads this thread and can ask on it. Answers come from
              the host, on the host&rsquo;s budget.
            </span>
          </div>
        ) : (
          thread.entries.map((entry) => <Entry key={entry.id} entry={entry} />)
        )}
      </div>

      <div className="chat-composer">
        {thread.busy && (
          <div className="chat-thinking">
            <span className="live-dot" />
            Answering {waitingOn(thread)}…
          </div>
        )}
        {error !== null && (
          <div className="chat-stopped" role="alert">
            {error}
          </div>
        )}
        <div className="chat-input-row">
          <input
            className="text-input chat-input"
            placeholder={thread.busy ? "Someone else is asking…" : "Ask on the shared thread…"}
            value={draft}
            disabled={thread.busy}
            aria-label="Ask on the shared thread"
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void send();
            }}
          />
          <button
            type="button"
            className="btn-primary"
            disabled={thread.busy || draft.trim() === ""}
            onClick={() => void send()}
          >
            Send
          </button>
        </div>
        <span className="muted chat-foot">
          Shared with every paired device · answered by the host, on the host&rsquo;s budget
        </span>
      </div>
    </>
  );
}
