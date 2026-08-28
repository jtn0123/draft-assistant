import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { errorMessage } from "../format";
import type { ChatOptions, ChatSessionSummary, ChatUsage } from "../types";
import { ChatSessions } from "./ChatSessions";
import { fromSession, newSessionId, nowSecs, toSession } from "./chatSession";
import { ChatSettings, UsageLine } from "./ChatSettings";
import {
  AUTO_QUESTION,
  loadOptions,
  loadPrefs,
  saveOptions,
  savePrefs,
  toHistory,
  type ChatPrefs,
  type Turn,
} from "./chatOptions";
import { Markdown } from "./Markdown";

// Answers take 15–40s with the whole board in context. They stream in as
// they are written, and the panel offers a way out, because a slow model
// call must never pin the panel while the pick clock is running.
const SUGGESTIONS = [
  AUTO_QUESTION,
  "What position am I weakest at?",
  "Who is likely gone before my next pick?",
  "Which flagged players are a real injury risk?",
];

type Busy = "ask" | "compact" | null;

export function Chat({
  open,
  onClose,
  currentPick,
  draftId,
  leagueName = "",
  onClock = false,
  onAutoAsk,
}: {
  open: boolean;
  onClose: () => void;
  /** The pick on the clock now; answers are stamped against it. */
  currentPick?: number;
  /** The draft conversations are filed under; without it nothing is saved. */
  draftId?: string;
  leagueName?: string;
  /** True while it is the user's turn in a live draft. */
  onClock?: boolean;
  /** Called when the panel asks by itself, so the app can show it. */
  onAutoAsk?: () => void;
}) {
  const [turns, setTurns] = useState<Turn[]>([]);
  const [question, setQuestion] = useState("");
  const [busy, setBusy] = useState<Busy>(null);
  // The answer so far while one streams; null before the first word.
  const [partial, setPartial] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [options, setOptions] = useState<ChatOptions>(loadOptions);
  const [prefs, setPrefs] = useState<ChatPrefs>(loadPrefs);
  const [usage, setUsage] = useState<ChatUsage | null>(null);
  const [session, setSession] = useState({ questions: 0, cost: 0 });
  // The saved-session bookkeeping: which file this conversation is, the
  // others on disk for this draft, and where the last save went.
  const [sessionId, setSessionId] = useState(newSessionId);
  const startedAt = useRef(nowSecs());
  const [sessions, setSessions] = useState<ChatSessionSummary[]>([]);
  const [savedTo, setSavedTo] = useState<string | null>(null);
  // True once something happened that is not on disk yet. Loading a saved
  // session must not count, or every reopen would rewrite its file.
  const dirty = useRef(false);
  const logRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  // Bumped on every ask, compact, cancel, and new chat. A result whose
  // generation no longer matches was abandoned in flight and is discarded.
  const generation = useRef(0);
  // Say "fast mode did not serve" once per session, not under every answer.
  const fastNoted = useRef(false);
  // The pick auto-ask last fired for, so one turn gets one question.
  const autoAsked = useRef<number | null>(null);

  useEffect(() => {
    if (open) inputRef.current?.focus();
  }, [open]);

  // Escape closes the panel from anywhere on the page. A handler on the panel
  // itself stops working the moment focus leaves it — clicking a suggestion
  // removes that button from the DOM and focus falls to <body>. While a
  // confirm dialog is open, Escape belongs to the dialog alone.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (document.querySelector("dialog[open]")) return;
      onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  // Keep the newest turn in view as answers land and as they stream.
  useEffect(() => {
    const log = logRef.current;
    if (log) log.scrollTop = log.scrollHeight;
  }, [turns, busy, partial]);

  const refreshSessions = async (id: string) => {
    try {
      setSessions(await api.listChatSessions(id));
    } catch {
      // Listing failing means saving will too; the save reports it.
    }
  };

  const openSession = async (id: string) => {
    if (!draftId || busy) return;
    try {
      const saved = await api.loadChatSession(draftId, id);
      generation.current += 1;
      fastNoted.current = false;
      dirty.current = false;
      setTurns(fromSession(saved));
      setSession({ questions: saved.questions, cost: saved.cost_usd });
      setSessionId(saved.id);
      startedAt.current = saved.started_at;
      setSavedTo(null);
      setUsage(null);
      setError(null);
      setPartial(null);
      setBusy(null);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  // The latest `openSession` for the restore effect, which runs once per
  // draft and must not re-run as the function closes over fresh state.
  const openSessionRef = useRef(openSession);
  useEffect(() => {
    openSessionRef.current = openSession;
  });

  // When a draft is loaded, pick up where the last conversation left off —
  // a reload or a relaunch should not cost the evening's answers. Runs once
  // per draft; StrictMode's dev-only double mount cancels the first run and
  // the second completes, which is why there is no "already restored" flag.
  useEffect(() => {
    if (!draftId) return;
    let cancelled = false;
    void (async () => {
      let list: ChatSessionSummary[];
      try {
        list = await api.listChatSessions(draftId);
      } catch {
        return;
      }
      if (cancelled) return;
      setSessions(list);
      if (list.length > 0 && !dirty.current) void openSessionRef.current(list[0].id);
    })();
    return () => {
      cancelled = true;
    };
  }, [draftId]);

  // Save after every completed answer, compaction or cancel: the file on
  // disk is whatever the panel shows once it has stopped moving.
  useEffect(() => {
    if (!draftId || busy !== null || !dirty.current) return;
    if (!turns.some((t) => t.role === "you" || t.role === "summary")) return;
    dirty.current = false;
    const snapshot = toSession({
      id: sessionId,
      draftId,
      leagueName,
      startedAt: startedAt.current,
      turns,
      questions: session.questions,
      cost: session.cost,
    });
    void api
      .saveChatSession(snapshot)
      .then((path) => {
        setSavedTo(path);
        return refreshSessions(draftId);
      })
      .catch((e: unknown) => setError(`Could not save this chat: ${errorMessage(e)}`));
  }, [turns, busy, draftId, leagueName, sessionId, session]);

  const updateOptions = (next: ChatOptions) => {
    if (next.fast !== options.fast) fastNoted.current = false;
    setOptions(next);
    saveOptions(next);
  };

  const updatePrefs = (next: ChatPrefs) => {
    setPrefs(next);
    savePrefs(next);
  };

  const overBudget = prefs.budget_usd > 0 && session.cost >= prefs.budget_usd;

  const record = (reply: ChatUsage) => {
    setUsage(reply);
    setSession((s) => ({
      questions: s.questions + 1,
      cost: s.cost + (reply.cost_usd ?? 0),
    }));
  };

  const fastNote = (reply: ChatUsage): Turn[] => {
    if (!options.fast || reply.fast_mode === "active" || fastNoted.current) return [];
    fastNoted.current = true;
    const reason = reply.fast_mode_reason ? ` (${reply.fast_mode_reason})` : "";
    return [{ role: "note", text: `Fast mode unavailable${reason} — answered at standard speed.` }];
  };

  const finish = (mine: number) => {
    if (mine !== generation.current) return;
    setBusy(null);
    setPartial(null);
    // Hand focus back so Enter keeps working after a suggestion click.
    inputRef.current?.focus();
  };

  const ask = async (text: string, auto = false) => {
    const trimmed = text.trim();
    if (!trimmed || busy || overBudget) return;
    const mine = ++generation.current;
    const history = toHistory(turns);
    setError(null);
    setQuestion("");
    setTurns((prev) => [
      ...prev,
      ...(auto ? [{ role: "note", text: "Asked automatically — you're on the clock." } as Turn] : []),
      { role: "you", text: trimmed },
    ]);
    setBusy("ask");
    setPartial(null);
    try {
      const reply = await api.chat(trimmed, history, options, (piece) => {
        if (mine !== generation.current) return;
        setPartial((sofar) => (sofar ?? "") + piece);
      });
      if (mine !== generation.current) return;
      dirty.current = true;
      setTurns((prev) => [
        ...prev,
        { role: "claude", text: reply.answer, asOfPick: reply.as_of?.pick },
        ...fastNote(reply.usage),
      ]);
      record(reply.usage);
    } catch (e) {
      if (mine !== generation.current) return;
      setError(errorMessage(e));
    } finally {
      finish(mine);
    }
  };

  // The latest `ask` for the auto-ask effect below, which must not re-run on
  // every render just because `ask` closes over fresh state.
  const askRef = useRef(ask);
  useEffect(() => {
    askRef.current = ask;
  });

  // When the user's pick comes up, ask the one question that matters —
  // once per pick, never on top of an answer in flight, never past budget.
  useEffect(() => {
    if (!prefs.auto_ask || !onClock || currentPick === undefined) return;
    if (busy !== null || overBudget) return;
    if (autoAsked.current === currentPick) return;
    autoAsked.current = currentPick;
    onAutoAsk?.();
    void askRef.current(AUTO_QUESTION, true);
  }, [prefs.auto_ask, onClock, currentPick, busy, overBudget, onAutoAsk]);

  const compact = async () => {
    if (busy) return;
    const mine = ++generation.current;
    setError(null);
    setBusy("compact");
    try {
      const reply = await api.chatCompact(toHistory(turns), options);
      if (mine !== generation.current) return;
      dirty.current = true;
      setTurns([
        { role: "summary", text: reply.answer },
        { role: "note", text: "Earlier conversation folded into the summary above." },
      ]);
      setUsage(reply.usage);
      setSession((s) => ({ ...s, cost: s.cost + (reply.usage.cost_usd ?? 0) }));
    } catch (e) {
      if (mine !== generation.current) return;
      setError(errorMessage(e));
    } finally {
      finish(mine);
    }
  };

  const cancel = () => {
    generation.current += 1;
    // Keep whatever had been written: a half answer beats none on the clock.
    const kept = partial?.trim();
    dirty.current = true;
    setTurns((prev) => [
      ...prev,
      ...(kept ? [{ role: "claude", text: kept, asOfPick: currentPick } as Turn] : []),
      { role: "note", text: kept ? "Cancelled — kept what was written so far." : "Cancelled — ask again whenever." },
    ]);
    setBusy(null);
    setPartial(null);
    inputRef.current?.focus();
  };

  const newChat = () => {
    generation.current += 1;
    fastNoted.current = false;
    dirty.current = false;
    setSessionId(newSessionId());
    startedAt.current = nowSecs();
    setSavedTo(null);
    setTurns([]);
    setUsage(null);
    setSession({ questions: 0, cost: 0 });
    setError(null);
    setBusy(null);
    setPartial(null);
    inputRef.current?.focus();
  };

  if (!open) return null;

  const asked = turns.filter((t) => t.role === "you").length;
  const canCompact = busy === null && asked >= 2;
  const canAsk = busy === null && !overBudget;

  const asOf = (pick: number | undefined) => {
    if (pick === undefined) return null;
    const since = currentPick !== undefined ? currentPick - pick : 0;
    return (
      <span className={`chat-asof muted${since > 0 ? " stale" : ""}`}>
        as of pick {pick}
        {since > 0 && ` · ${since} pick${since === 1 ? "" : "s"} since`}
      </span>
    );
  };

  return (
    <aside className="chat" aria-label="Ask Claude about this draft">
      <header className="chat-head">
        <h3>Ask Claude</h3>
        <div className="chat-head-actions">
          <button
            className="ghost small"
            onClick={newChat}
            disabled={turns.length === 0 && busy === null}
            title="Start over with an empty conversation"
          >
            New chat
          </button>
          <button
            className="ghost small"
            onClick={() => void compact()}
            disabled={!canCompact}
            title="Fold the conversation into a short summary so it stops costing its full length. Takes a minute or two."
          >
            Compact
          </button>
          <button className="ghost" onClick={onClose} aria-label="Close chat" title="Close (Esc)">
            ✕
          </button>
        </div>
      </header>

      <ChatSettings
        options={options}
        onChange={updateOptions}
        prefs={prefs}
        onPrefsChange={updatePrefs}
        disabled={busy !== null}
      />

      {draftId && (
        <ChatSessions
          sessions={sessions}
          currentId={sessionId}
          savedTo={savedTo}
          disabled={busy !== null}
          onSelect={(id) => void openSession(id)}
        />
      )}

      <div className="chat-log" ref={logRef}>
        {turns.length === 0 && !busy && (
          <div className="chat-empty">
            <p className="muted">
              Claude sees the whole board with points, VORP, tiers, ADP and survival
              odds, every roster, recent picks, and the app&apos;s own recommendation
              — and remembers this conversation.
            </p>
            {SUGGESTIONS.map((s) => (
              <button
                key={s}
                className="chat-suggestion"
                onClick={() => void ask(s)}
                disabled={!canAsk}
              >
                {s}
              </button>
            ))}
          </div>
        )}
        {turns.map((turn, i) =>
          turn.role === "note" ? (
            <p key={i} className="chat-note muted">
              {turn.text}
            </p>
          ) : turn.role === "summary" ? (
            <div key={i} className="chat-turn summary">
              <span className="chat-role">Summary so far</span>
              <p>{turn.text}</p>
            </div>
          ) : turn.role === "claude" ? (
            <div key={i} className="chat-turn claude">
              <span className="chat-role">Claude</span>
              <Markdown text={turn.text} />
              {asOf(turn.asOfPick)}
            </div>
          ) : (
            <div key={i} className="chat-turn you">
              <span className="chat-role">You</span>
              <p>{turn.text}</p>
            </div>
          ),
        )}
        {busy === "ask" && partial !== null && (
          <div className="chat-turn claude streaming">
            <span className="chat-role">Claude</span>
            <Markdown text={partial} />
          </div>
        )}
        {busy && partial === null && (
          <div className="chat-turn claude pending" aria-live="polite">
            <span className="chat-role">Claude</span>
            <p className="muted">
              {busy === "ask"
                ? "Thinking… (the answer appears as it is written; usually 15–40 seconds)"
                : "Compacting the conversation — this usually takes a minute or two…"}
            </p>
          </div>
        )}
        {error && (
          <div className="chat-error" role="alert">
            {error}
          </div>
        )}
        {overBudget && (
          <p className="chat-note muted" role="status">
            Session budget of ${prefs.budget_usd.toFixed(2)} reached (${session.cost.toFixed(2)}{" "}
            spent) — raise it in Settings or start a new chat.
          </p>
        )}
      </div>

      <UsageLine usage={usage} questions={session.questions} cost={session.cost} />

      <div className="chat-input">
        <textarea
          ref={inputRef}
          value={question}
          rows={2}
          placeholder="Ask about the draft…"
          aria-label="Your question"
          onChange={(e) => setQuestion(e.target.value)}
          onKeyDown={(e) => {
            // Enter sends; Shift+Enter is a newline.
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void ask(question);
            }
          }}
        />
        {busy ? (
          <button className="ghost" onClick={cancel}>
            Cancel
          </button>
        ) : (
          <button onClick={() => void ask(question)} disabled={!question.trim() || !canAsk}>
            Ask
          </button>
        )}
      </div>
    </aside>
  );
}
