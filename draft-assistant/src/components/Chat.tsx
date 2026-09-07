// The Ask Claude panel: model and effort pickers, saved conversations, the
// thread, and the composer. Every answer is a real Messages API call against
// the current board — there is no canned content here.

import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import type { ChatSettings } from "../chat-types";
import { formatUsd, overBudget, setChatBudget, useChatBudget } from "../chatCost";
import { chatScope } from "../chatSessions";
import { describeError } from "../errorText";
import type { Screen } from "../prefs";
import { ChatControls } from "./ChatControls";
import { ChatKeyForm } from "./ChatKeyForm";
import { ChatSessionBar } from "./ChatSessionBar";
import { ChatThread } from "./ChatThread";
import { SharedChat } from "./SharedChat";
import { useChatSessions, type ChatSnapshot } from "./useChatSessions";
import { resumeOrBegin, useChatThread } from "./useChatThread";
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

/** When the session a question belongs to began; a session that has not
 *  been filed yet starts now. Kept outside the component because it reads
 *  the clock, which render must not. */
function sessionStartedAt(
  sessions: ReadonlyArray<{ id: string; startedAt: number }>,
  sessionId: string,
): number {
  return sessions.find((s) => s.id === sessionId)?.startedAt ?? Date.now();
}

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
  // The conversation this panel opens with: the one whose question is still
  // out, the newest one stored for this screen and league, or a fresh one.
  // Read while the state below is initialised, so a reopened thread paints
  // once rather than appearing after an empty one.
  const [{ opening, pending }] = useState(() => resumeOrBegin(scope));
  // How a finished turn is filed. Filled in below, once the saved-chats hook
  // exists; it needs the thread's own `show` and `clear` first.
  const file = useRef<(snapshot: ChatSnapshot) => void>(() => undefined);
  const thread = useChatThread(scope, opening, pending, file);
  const { setScreenSpend } = thread;
  const [draft, setDraft] = useState("");
  const [askingNew, setAskingNew] = useState(false);
  const [showKeyForm, setShowKeyForm] = useState(false);
  const [suggestions, setSuggestions] = useState<string[]>([]);
  /// Bumped after the key is saved, to re-read whether one is stored.
  const [settingsToken, setSettingsToken] = useState(0);
  const budget = useChatBudget();

  const sessions = useChatSessions({
    scope,
    opening,
    onOpen: (chat) => {
      thread.show(chat);
      setAskingNew(false);
    },
    onClear: thread.clear,
  });
  // A finished turn is filed the way a saved conversation is, whichever
  // render it lands in.
  useEffect(() => {
    file.current = sessions.save;
  }, [sessions.save]);

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
  }, [settingsToken, scope, sharedOnly, setScreenSpend]);

  // A phone's question is answered on this machine's budget and written to
  // the same tally, but it never passes through this panel, so the screen
  // figure here stood still while the phones spent. Every answer on the
  // shared thread re-reads the tally.
  useEffect(() => {
    let live = true;
    const pending = api.onSharedChat((next) => {
      if (!live || next.screen !== screen || next.busy) return;
      api
        .chatSettings()
        .then((settings) => {
          if (live) setScreenSpend(settings.spend_usd[scope] ?? 0);
        })
        .catch(() => {
          // The figure stays as it was until the next answer.
        });
    });
    return () => {
      live = false;
      void pending.then((off) => off());
    };
  }, [screen, scope, setScreenSpend]);

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

  // Fable 5 cannot turn thinking off, so the picked level may not be legal for
  // the picked model. Derive the effective one rather than correcting state
  // after render — switching model must never send a level the API rejects.
  const allowedEfforts = useMemo(
    () => settings?.efforts[model] ?? ["Low", "Medium", "High", "xhigh", "Max"],
    [settings, model],
  );
  const activeEffort = allowedEfforts.includes(effort) ? effort : DEFAULT_EFFORT;
  const { entries, spend, sending, screenSpend } = thread;
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

  const send = (text: string) => {
    const question = text.trim();
    if (!question || sending || showKeyForm) return;
    setDraft("");
    const startedAt = sessionStartedAt(sessions.sessions, sessions.sessionId);
    thread.send(
      question,
      { screen, model, effort: activeEffort },
      { id: sessions.sessionId, startedAt },
    );
  };

  // Switching route re-reads settings, which also decides whether the key
  // form needs to show.
  const pickProvider = (id: "api" | "claude_code") => {
    api
      .setChatProvider(id)
      .then(() => setSettingsToken((n) => n + 1))
      .catch((e: unknown) => thread.add({ kind: "error", lines: [describeError(e)] }));
  };

  const startFresh = () => {
    thread.clear();
    sessions.startNew();
    setAskingNew(false);
  };

  const carryThread = () => {
    // A separate file from here on; the turns above it stay in both.
    sessions.startNew();
    thread.add({ kind: "divider", lines: ["New chat · carried the thread above as context"] });
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
          <ChatThread
            entries={entries}
            compact={compact}
            sending={sending}
            keyForm={
              showKeyForm ? (
                <ChatKeyForm
                  hint={settings?.key_hint ?? null}
                  store={settings?.key_store ?? null}
                  onSaved={() => setSettingsToken((n) => n + 1)}
                />
              ) : null
            }
            emptyTitle="New chat"
            emptyNote={EMPTY_NOTE[screen]}
          />

          <div className="chat-composer">
            {sending && (
              <div className="chat-thinking">
                <span className="live-dot" />
                {THINKING_NOTE[activeEffort] ?? "Thinking…"}
                <button
                  type="button"
                  className="link-btn chat-cancel"
                  aria-label="Cancel the answer"
                  title="Stop this answer. What has arrived so far is kept."
                  onClick={() => thread.cancel(screen)}
                >
                  Cancel
                </button>
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
                    onClick={() => send(text)}
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
                  if (e.key === "Enter") send(draft);
                }}
                aria-label="Ask Claude"
              />
              <button
                type="button"
                className="btn-primary"
                disabled={composerOff || draft.trim() === ""}
                onClick={() => send(draft)}
              >
                Send
              </button>
            </div>
            <span className="muted chat-foot">
              {model} · {note?.[1] ?? activeEffort} ·{" "}
              {settings?.provider === "claude_code" ? "via Claude Code" : "via the API"} · reads
              your board, never writes to your league
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
