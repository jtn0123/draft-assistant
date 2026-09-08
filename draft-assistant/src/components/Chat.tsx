// The Ask AI panel: model and effort pickers, saved conversations, the
// thread, and the composer. Every answer is a real Messages API call against
// the current board — there is no canned content here.

import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import type { ChatSettings } from "../chat-types";
import { chatScope } from "../chatSessions";
/** The screens a question can be asked about. The window has a third,
 *  Projections, which is the season's own numbers and asks about the season:
 *  the shell hands this panel "season" for it. */
type AskScreen = "draft" | "season";
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

/** What the selected AI can see, per screen, for the empty thread. */
const EMPTY_NOTE: Record<AskScreen, string> = {
  draft:
    "Your selected AI sees your live board, your roster and the clock. Ask anything about who to take.",
  season:
    "Your selected AI sees this week's matchup, your roster and the waiver wire. Ask anything about who to start.",
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
  screen: AskScreen;
  /** Which league the questions are about. Conversations are filed under it,
   *  so switching leagues opens a thread about the board now on screen. */
  leagueId: string;
  contextNote: string;
  /** Follower mode: the host owns the local composer and provider credentials,
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
        // Cache the backend's estimated cost for this screen and league,
        // using the same scope as its saved conversations.
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

  // A phone's question uses this machine's provider and updates the same
  // estimated cost tally without passing through this panel. Re-read that
  // tally after each shared answer so the displayed estimate stays current.
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

  // Fable 5.1 cannot turn thinking off, so the picked level may not be legal for
  // the picked model. Derive the effective one rather than correcting state
  // after render — switching model must never send a level the API rejects.
  const allowedEfforts = useMemo(
    () => settings?.efforts[model] ?? ["Low", "Medium", "High", "xhigh", "Max"],
    [settings, model],
  );
  const activeEffort = allowedEfforts.includes(effort)
    ? effort
    : allowedEfforts.includes(DEFAULT_EFFORT)
      ? DEFAULT_EFFORT
      : (allowedEfforts[0] ?? DEFAULT_EFFORT);
  const { entries, spend, sending, screenSpend } = thread;
  const isOpenAI = model.startsWith("GPT-");
  const needsKey = !isOpenAI && showKeyForm;
  const missingCodex = isOpenAI && settings?.codex_available === false;
  const route = isOpenAI ? "codex" : (settings?.provider ?? null);

  // What the backend has written of the answer so far. The API route hands it
  // over as it arrives; a CLI route says nothing until it says everything, and
  // this stays empty there.
  const [arriving, setArriving] = useState("");
  useEffect(() => {
    let live = true;
    const pending = api.onChatProgress((progress) => {
      // Both screens' questions are answered through one command, so a season
      // answer must not be painted into the draft panel.
      if (live && progress.screen === screen) setArriving(progress.text);
    });
    return () => {
      live = false;
      void pending.then((stop) => stop());
    };
  }, [screen]);
  // Nothing clears this when the answer lands: it is only ever rendered while
  // the turn is in flight, the finished turn carries the same text rendered
  // properly, and the next question clears it before asking.

  const send = (text: string) => {
    const question = text.trim();
    if (!question || sending || needsKey || missingCodex) return;
    setDraft("");
    setArriving("");
    const startedAt = sessionStartedAt(sessions.sessions, sessions.sessionId);
    thread.send(
      question,
      { screen, model, effort: activeEffort },
      { id: sessions.sessionId, startedAt },
    );
  };

  const startFresh = () => {
    if (sending) return;
    thread.clear();
    sessions.startNew();
    setAskingNew(false);
  };

  const carryThread = () => {
    if (sending) return;
    // A separate file from here on; the turns above it stay in both.
    sessions.startNew();
    thread.add({ kind: "divider", lines: ["New chat · carried the thread above as context"] });
    setAskingNew(false);
  };

  const note = settings?.notes[activeEffort];
  const composerOff = needsKey || sending || missingCodex;

  return (
    <aside className="chat" ref={useRevealOnMount<HTMLElement>()}>
      <div className="chat-head">
        <div className="chat-head-titles">
          <span className="chat-title">Ask AI</span>
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
            disabled={shared || sending || entries.length === 0}
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
          provider={route}
          disabled={sending}
          onOpen={sessions.open}
          onDelete={sessions.remove}
        />
      )}

      {askingNew && !shared && (
        <div className="chat-newbar">
          <span className="small">Start a new chat:</span>
          <button
            type="button"
            className="btn-primary btn-row"
            disabled={sending}
            onClick={startFresh}
          >
            Fresh start
          </button>
          <button
            type="button"
            className="btn-ghost btn-row"
            disabled={sending}
            onClick={carryThread}
          >
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
              needsKey ? (
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
            {sending && arriving !== "" && (
              <div className="chat-arriving" aria-live="polite" aria-atomic="false">
                {arriving}
              </div>
            )}
            {sending && (
              <div className="chat-thinking">
                <span className="live-dot" />
                {arriving === "" ? (THINKING_NOTE[activeEffort] ?? "Thinking…") : "Writing…"}
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
            {missingCodex && (
              <div className="chat-stopped" role="status">
                Install Codex and sign in with ChatGPT on this Mac to use {model}.
              </div>
            )}
            {!needsKey && entries.length === 0 && suggestions.length > 0 && (
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
                placeholder={needsKey ? "Waiting on the key above…" : "Ask about the board…"}
                value={draft}
                disabled={composerOff}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") send(draft);
                }}
                aria-label="Ask AI"
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
              {route === "codex"
                ? "via ChatGPT (Codex)"
                : route === "claude_code"
                  ? "via Claude Code"
                  : "via the API"}{" "}
              · reads your board, never writes to your league
              {!isOpenAI && settings?.has_key === true && (
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
