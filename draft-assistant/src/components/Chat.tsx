// The Ask Claude panel: model and effort pickers, saved conversations, the
// thread, and the composer. Every answer is a real Messages API call against
// the current board — there is no canned content here.

import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import type { ChatMessage, ChatSettings, ThreadEntry } from "../chat-types";
import { formatUsd, overBudget, setChatBudget, useChatBudget } from "../chatCost";
import { chatScope } from "../chatSessions";
import { describeError } from "../errorText";
import type { Screen } from "../prefs";
import { ChatControls } from "./ChatControls";
import { ChatKeyForm } from "./ChatKeyForm";
import { ChatSessionBar } from "./ChatSessionBar";
import { Markdown } from "./Markdown";
import { SharedChat } from "./SharedChat";
import { beginChat, useChatSessions } from "./useChatSessions";
import { useRevealOnMount } from "./useRevealOnMount";

// Ship with this chunk, not with the window. live.css owns the pulsing dot
// this panel borrows while an answer is on its way.
import "../chat.css";
import "../live.css";

const DEFAULT_MODEL = "Opus 5";
const DEFAULT_EFFORT = "High";

/** "high effort" / "no thinking" — the effort as the context line names it. */
function effortTag(level: string): string {
  return level === "Off" ? "no thinking" : `${level.toLowerCase()} effort`;
}

/** What Claude can see, per screen, for the empty thread. */
const EMPTY_NOTE: Record<Screen, string> = {
  draft: "Claude sees your live board, your roster and the clock — ask anything about who to take.",
  season:
    "Claude sees this week's matchup, your roster and the waiver wire — ask anything about who to start.",
};

/** Thinking copy while a request is in flight, per effort level. */
const THINKING_NOTE: Record<string, string> = {
  Off: "Answering straight from the board…",
  Low: "Checking the board…",
  Medium: "Checking the numbers…",
  High: "Thinking it through…",
  xhigh: "Working through your next few picks…",
  Max: "Simulating the rest of the round…",
};

export function Chat({
  screen,
  leagueId,
  contextNote,
  sharedOnly = false,
  onClose,
}: {
  screen: Screen;
  /** Which league the questions are about. Conversations are filed under it,
   *  so switching leagues opens a thread about the board now on screen. */
  leagueId: string;
  contextNote: string;
  /** Follower mode: the host owns the local composer, its key and its budget,
   *  so the panel offers the shared thread and nothing else. */
  sharedOnly?: boolean;
  onClose: () => void;
}) {
  const scope = chatScope(screen, leagueId);
  const [settings, setSettings] = useState<ChatSettings | null>(null);
  const [model, setModel] = useState(DEFAULT_MODEL);
  const [effort, setEffort] = useState(DEFAULT_EFFORT);
  const [compact, setCompact] = useState(false);
  // Whether the pinned shared thread is the one on screen rather than one of
  // this Mac's saved conversations.
  const [shared, setShared] = useState(sharedOnly);
  // The conversation this panel opens with: the newest one stored for this
  // screen and league, or a fresh one. Read while the state below is
  // initialised, so a reopened thread paints once rather than appearing after
  // an empty one.
  const [opening] = useState(() => beginChat(scope));
  const [entries, setEntries] = useState<ThreadEntry[]>(() => opening.reopened?.entries ?? []);
  const [history, setHistory] = useState<ChatMessage[]>(() => opening.reopened?.history ?? []);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [askingNew, setAskingNew] = useState(false);
  const [showKeyForm, setShowKeyForm] = useState(false);
  const [suggestions, setSuggestions] = useState<string[]>([]);
  /// What this conversation has asked and cost, restored with a saved one.
  const [spend, setSpend] = useState(() => ({
    questions: opening.reopened?.questions ?? 0,
    costUsd: opening.reopened?.costUsd ?? 0,
  }));
  /// Bumped after the key is saved, to re-read whether one is stored.
  const [settingsToken, setSettingsToken] = useState(0);
  /// What every conversation on this screen has cost together — the total the
  /// backend's cap is actually checked against, which this one chat's is not.
  const [screenSpend, setScreenSpend] = useState(0);
  const nextId = useRef(Math.max(1, ...(opening.reopened?.entries ?? []).map((e) => e.id + 1)));
  const threadRef = useRef<HTMLDivElement>(null);
  const budget = useChatBudget();

  const clearThread = () => {
    setEntries([]);
    setHistory([]);
    setSpend({ questions: 0, costUsd: 0 });
  };

  const sessions = useChatSessions({
    scope,
    opening,
    onOpen: (chat) => {
      // Ids come back with the conversation, so a new turn cannot collide
      // with one that was stored.
      nextId.current = Math.max(1, ...chat.entries.map((e) => e.id + 1));
      setEntries(chat.entries);
      setHistory(chat.history);
      setSpend({ questions: chat.questions, costUsd: chat.costUsd });
      setAskingNew(false);
    },
    onClear: clearThread,
  });

  // Reloaded on mount and after the key changes; state is set from the
  // promise callback so the effect body stays synchronous-free.
  useEffect(() => {
    let cancelled = false;
    api
      .chatSettings()
      .then((next) => {
        if (cancelled) return;
        setSettings(next);
        setShowKeyForm(!sharedOnly && next.provider === "api" && !next.has_key);
        // The backend holds the cap and the running total it is checked
        // against; the stored copy here is only a cache of them.
        setChatBudget(next.budget_usd);
        // Keyed by screen *and* league, the same scope the conversations are
        // filed under: a cap drawn down by another league's questions is not
        // this league's cap.
        setScreenSpend(next.spend_usd[scope] ?? 0);
      })
      .catch(() => {
        // Without settings the panel still renders, just with defaults.
        if (!cancelled) setSettings(null);
      });
    return () => {
      cancelled = true;
    };
  }, [settingsToken, scope, sharedOnly]);

  useEffect(() => {
    let cancelled = false;
    api
      .chatSuggestions(screen)
      .then((next) => {
        if (!cancelled) setSuggestions(next);
      })
      .catch(() => {
        if (!cancelled) setSuggestions([]);
      });
    return () => {
      cancelled = true;
    };
  }, [screen]);

  // Keep the newest turn in view as the thread grows.
  useEffect(() => {
    threadRef.current?.scrollTo({ top: threadRef.current.scrollHeight });
  }, [entries, sending]);

  // Fable 5 cannot turn thinking off, so the picked level may not be legal for
  // the picked model. Derive the effective one rather than correcting state
  // after render — switching model must never send a level the API rejects.
  const allowedEfforts = useMemo(
    () => settings?.efforts[model] ?? ["Low", "Medium", "High", "xhigh", "Max"],
    [settings, model],
  );
  const activeEffort = allowedEfforts.includes(effort) ? effort : DEFAULT_EFFORT;
  // What the cap is actually measured against: everything this screen's chats
  // have cost together. This conversation's own total is the floor, because
  // the screen figure is only as fresh as the last answer — a turn charged
  // here before the backend reported back is still money spent.
  const spentOnScreen = Math.max(spend.costUsd, screenSpend);
  // A warning, not a lock: the backend holds the real cap and charges the
  // Claude Code route nothing. Disabling the composer here would stop
  // questions over money that was never spent.
  const nearingCap = overBudget(spentOnScreen, budget);

  /** Keep the cap the panel warns on and the cap the backend enforces the
   *  same number. A backend that refuses the write still warns correctly. */
  const pickBudget = (next: number) => {
    // A cap the local half will not take is one the backend refuses too, so
    // it never goes over the wire.
    if (!setChatBudget(next)) return;
    api.setChatBudget(next).catch(() => {
      // Not stored for next time; this session still uses it.
    });
  };

  const add = (entry: Omit<ThreadEntry, "id">) => {
    const id = nextId.current++;
    setEntries((prev) => [...prev, { ...entry, id }]);
  };

  const send = async (text: string) => {
    const question = text.trim();
    if (!question || sending || showKeyForm) return;
    setDraft("");
    // The turns are built here rather than only in state, so the conversation
    // can be filed the moment it stops moving without waiting for a render.
    const asked = [...entries, { id: nextId.current++, kind: "me" as const, lines: [question] }];
    setEntries(asked);
    const outgoing: ChatMessage[] = [...history, { role: "user", content: question }];
    setHistory(outgoing);
    setSending(true);
    try {
      const reply = await api.askClaude({
        screen,
        model,
        effort: activeEffort,
        messages: outgoing,
      });
      const answered = [
        ...asked,
        {
          id: nextId.current++,
          kind: "claude" as const,
          label: reply.refused ? "Declined" : undefined,
          lines: reply.text.split("\n\n").filter((l) => l.trim() !== ""),
        },
      ];
      const thread = [...outgoing, { role: "assistant", content: reply.text }];
      // The backend prices the turn, so a Claude Code answer adds nothing and
      // the panel's total is the one the cap is actually checked against.
      const spent = {
        questions: spend.questions + 1,
        costUsd: spend.costUsd + reply.cost_usd,
      };
      setEntries(answered);
      setHistory(thread);
      setSpend(spent);
      setScreenSpend(reply.screen_spend_usd);
      sessions.save({ entries: answered, history: thread, ...spent });
    } catch (e) {
      // The failed turn must not stay in history, or every retry resends it.
      const failed = [
        ...asked,
        { id: nextId.current++, kind: "error" as const, lines: [describeError(e)] },
      ];
      setHistory(history);
      setEntries(failed);
      sessions.save({ entries: failed, history, ...spend });
    } finally {
      setSending(false);
    }
  };

  // Switching route re-reads settings, which also decides whether the key
  // form needs to show.
  const pickProvider = (id: "api" | "claude_code") => {
    api
      .setChatProvider(id)
      .then(() => setSettingsToken((n) => n + 1))
      .catch((e: unknown) => add({ kind: "error", lines: [describeError(e)] }));
  };

  const startFresh = () => {
    clearThread();
    sessions.startNew();
    setAskingNew(false);
  };

  const carryThread = () => {
    // A separate file from here on; the turns above it stay in both.
    sessions.startNew();
    add({ kind: "divider", lines: ["New chat · carried the thread above as context"] });
    setAskingNew(false);
  };

  const note = settings?.notes[activeEffort];
  const composerOff = showKeyForm || sending;

  return (
    <aside className="chat" ref={useRevealOnMount<HTMLElement>()}>
      <div className="chat-head">
        <div className="chat-head-titles">
          <span className="chat-title">Ask Claude</span>
          <span className="muted small ellipsis">
            {contextNote} · {model} · {effortTag(activeEffort)}
          </span>
        </div>
        <div className="chat-head-actions">
          <button
            type="button"
            className={compact ? "btn-ghost btn-row is-on" : "btn-ghost btn-row"}
            onClick={() => setCompact((c) => !c)}
            title={compact ? "Switch to roomier spacing" : "Tighten the thread"}
          >
            {compact ? "Cozy" : "Compact"}
          </button>
          <button
            type="button"
            className="btn-ghost btn-row"
            onClick={() => setAskingNew(true)}
            disabled={shared || entries.length === 0}
          >
            New
          </button>
          <button type="button" className="link-btn" onClick={onClose}>
            Close
          </button>
        </div>
      </div>

      {!shared && (
        <ChatControls
          settings={settings}
          models={settings?.models ?? [DEFAULT_MODEL]}
          model={model}
          onModel={setModel}
          efforts={allowedEfforts}
          effort={activeEffort}
          onEffort={setEffort}
          onProvider={pickProvider}
        />
      )}

      {!sharedOnly && (
        <ChatSessionBar
          sessions={sessions.sessions}
          currentId={sessions.sessionId}
          shared={shared}
          onShared={setShared}
          saved={sessions.saved}
          spent={spend.costUsd}
          screenSpent={screenSpend}
          budget={budget}
          provider={settings?.provider ?? null}
          disabled={sending}
          onOpen={sessions.open}
          onDelete={sessions.remove}
          onBudget={pickBudget}
        />
      )}

      {askingNew && !shared && (
        <div className="chat-newbar">
          <span className="small">Start a new chat —</span>
          <button type="button" className="btn-primary btn-row" onClick={startFresh}>
            Fresh start
          </button>
          <button type="button" className="btn-ghost btn-row" onClick={carryThread}>
            Carry this thread
          </button>
          <button
            type="button"
            className="link-btn chat-newbar-cancel"
            onClick={() => setAskingNew(false)}
          >
            Cancel
          </button>
        </div>
      )}

      {shared ? (
        <SharedChat screen={screen} compact={compact} />
      ) : (
        <>
          <div className={compact ? "chat-thread is-compact" : "chat-thread"} ref={threadRef}>
            {showKeyForm ? (
              <ChatKeyForm
                hint={settings?.key_hint ?? null}
                store={settings?.key_store ?? null}
                onSaved={() => setSettingsToken((n) => n + 1)}
              />
            ) : entries.length === 0 ? (
              <div className="chat-empty">
                <span className="chat-empty-title">New chat</span>
                <span className="mid small">{EMPTY_NOTE[screen]}</span>
              </div>
            ) : (
              entries.map((entry) => (
                <div className={`msg is-${entry.kind}`} key={entry.id}>
                  {entry.label && <span className="msg-label">{entry.label}</span>}
                  {entry.kind === "claude" ? (
                    <Markdown text={entry.lines.join("\n\n")} />
                  ) : (
                    entry.lines.map((line, i) => (
                      <span className="msg-line" key={i}>
                        {line}
                      </span>
                    ))
                  )}
                </div>
              ))
            )}
          </div>

          <div className="chat-composer">
            {sending && (
              <div className="chat-thinking">
                <span className="live-dot" />
                {THINKING_NOTE[activeEffort] ?? "Thinking…"}
              </div>
            )}
            {nearingCap && (
              <div className="chat-stopped" role="status">
                This screen&rsquo;s chats have spent {formatUsd(spentOnScreen)} of their{" "}
                {formatUsd(budget)} budget — the next question may be refused. Raise the budget
                above.
              </div>
            )}
            {!showKeyForm && entries.length === 0 && suggestions.length > 0 && (
              <div className="chat-suggestions">
                {suggestions.map((text) => (
                  <button
                    key={text}
                    type="button"
                    className="chat-suggestion"
                    disabled={composerOff}
                    onClick={() => void send(text)}
                  >
                    {text}
                  </button>
                ))}
              </div>
            )}
            <div className="chat-input-row">
              <input
                className="text-input chat-input"
                /* The form above is the one place a key is added; the composer
               points at it rather than asking a second time. */
                placeholder={showKeyForm ? "Waiting on the key above…" : "Ask about the board…"}
                value={draft}
                disabled={composerOff}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void send(draft);
                }}
                aria-label="Ask Claude"
              />
              <button
                type="button"
                className="btn-primary"
                disabled={composerOff || draft.trim() === ""}
                onClick={() => void send(draft)}
              >
                Send
              </button>
            </div>
            <span className="muted chat-foot">
              {model} · {note?.[1] ?? activeEffort} ·{" "}
              {settings?.provider === "claude_code" ? "via Claude Code" : "via the API"} · reads
              your board, never writes to Sleeper
              {settings?.has_key === true && (
                <>
                  {" · "}
                  <button
                    type="button"
                    className="link-btn"
                    onClick={() => setShowKeyForm((s) => !s)}
                  >
                    {showKeyForm ? "cancel" : "change key"}
                  </button>
                </>
              )}
            </span>
          </div>
        </>
      )}
    </aside>
  );
}
