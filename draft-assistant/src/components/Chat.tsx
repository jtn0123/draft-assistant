// The Ask Claude panel: model and effort pickers, the thread, and the
// composer. Every answer is a real Messages API call against the current
// board — there is no canned content here.

import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import type { ChatMessage, ChatSettings, ThreadEntry } from "../chat-types";
import type { Screen } from "./Header";

const DEFAULT_MODEL = "Opus 5";
const DEFAULT_EFFORT = "High";

/** The two ways an answer can reach Claude. */
const PROVIDERS: [id: "claude_code" | "api", name: string, title: string][] = [
  [
    "claude_code",
    "Claude Code",
    "Runs the Claude Code CLI installed on this Mac, signed in with your Claude subscription — no API key needed",
  ],
  ["api", "API key", "Calls the Anthropic API directly with the key stored in this app"],
];

/** Model-button tooltips, from the design. */
const MODEL_TITLE: Record<string, string> = {
  "Opus 5": "Claude Opus 5 — adaptive thinking, supports all five effort levels",
  "Fable 5":
    "Claude Fable 5 — Mythos-class; thinking can't be turned off, effort is the only depth control",
};

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

function ApiKeyPrompt({ hint, onSaved }: { hint: string | null; onSaved: () => void }) {
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.setApiKey(key.trim());
      setKey("");
      onSaved();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="chat-key">
      <span className="chat-key-title">
        {hint === null ? "Add an Anthropic API key" : "Replace the stored key"}
      </span>
      <span className="mid small">
        {hint === null
          ? "Ask Claude sends your board to the Anthropic API. The key is stored locally in this app's data directory and goes nowhere else."
          : `Currently using ${hint}.`}
      </span>
      <input
        className="text-input"
        type="password"
        value={key}
        placeholder="sk-ant-…"
        onChange={(e) => setKey(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && key.trim()) void save();
        }}
        aria-label="Anthropic API key"
      />
      <button type="button" className="btn-primary" disabled={!key.trim() || busy} onClick={save}>
        {busy ? "Saving…" : "Save key"}
      </button>
      {error && <div className="error">{error}</div>}
    </div>
  );
}

export function Chat({
  screen,
  contextNote,
  onClose,
}: {
  screen: Screen;
  contextNote: string;
  onClose: () => void;
}) {
  const [settings, setSettings] = useState<ChatSettings | null>(null);
  const [model, setModel] = useState(DEFAULT_MODEL);
  const [effort, setEffort] = useState(DEFAULT_EFFORT);
  const [compact, setCompact] = useState(false);
  const [entries, setEntries] = useState<ThreadEntry[]>([]);
  const [history, setHistory] = useState<ChatMessage[]>([]);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [askingNew, setAskingNew] = useState(false);
  const [showKeyForm, setShowKeyForm] = useState(false);
  const [suggestions, setSuggestions] = useState<string[]>([]);
  /// Bumped after the key is saved, to re-read whether one is stored.
  const [settingsToken, setSettingsToken] = useState(0);
  const nextId = useRef(1);
  const threadRef = useRef<HTMLDivElement>(null);

  // Reloaded on mount and after the key changes; state is set from the
  // promise callback so the effect body stays synchronous-free.
  useEffect(() => {
    let cancelled = false;
    api
      .chatSettings()
      .then((next) => {
        if (cancelled) return;
        setSettings(next);
        setShowKeyForm(next.provider === "api" && !next.has_key);
      })
      .catch(() => {
        // Without settings the panel still renders, just with defaults.
        if (!cancelled) setSettings(null);
      });
    return () => {
      cancelled = true;
    };
  }, [settingsToken]);

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

  const add = (entry: Omit<ThreadEntry, "id">) => {
    const id = nextId.current++;
    setEntries((prev) => [...prev, { ...entry, id }]);
  };

  const send = async (text: string) => {
    const question = text.trim();
    if (!question || sending) return;
    setDraft("");
    add({ kind: "me", lines: [question] });
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
      add({
        kind: "claude",
        label: reply.refused ? "Declined" : undefined,
        lines: reply.text.split("\n\n").filter((l) => l.trim() !== ""),
      });
      setHistory([...outgoing, { role: "assistant", content: reply.text }]);
    } catch (e) {
      // The failed turn must not stay in history, or every retry resends it.
      setHistory(history);
      add({ kind: "error", lines: [String(e)] });
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
      .catch((e: unknown) => add({ kind: "error", lines: [String(e)] }));
  };

  const startFresh = () => {
    setEntries([]);
    setHistory([]);
    setAskingNew(false);
  };

  const carryThread = () => {
    add({ kind: "divider", lines: ["New chat · carried the thread above as context"] });
    setAskingNew(false);
  };

  const note = settings?.notes[activeEffort];

  return (
    <aside className="chat">
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
            disabled={entries.length === 0}
          >
            New
          </button>
          <button type="button" className="link-btn" onClick={onClose}>
            Close
          </button>
        </div>
      </div>

      <div className="chat-controls">
        <div className="segmented" role="group" aria-label="Model">
          {(settings?.models ?? [DEFAULT_MODEL]).map((name) => (
            <button
              key={name}
              type="button"
              className={name === model ? "seg is-on" : "seg"}
              onClick={() => setModel(name)}
              title={MODEL_TITLE[name]}
              aria-pressed={name === model}
            >
              {name}
            </button>
          ))}
        </div>
        <span className="muted chat-model-note">
          {model === "Fable 5" ? "thinking always on" : "adaptive thinking"}
        </span>
        <span className="label chat-effort-label">Effort</span>
        <div className="segmented" role="group" aria-label="Effort">
          {allowedEfforts.map((level) => (
            <button
              key={level}
              type="button"
              className={level === activeEffort ? "seg is-on" : "seg"}
              onClick={() => setEffort(level)}
              title={settings?.notes[level]?.[0]}
              aria-pressed={level === activeEffort}
            >
              {level}
            </button>
          ))}
        </div>
        {settings?.cli_available && (
          <>
            <span className="label chat-effort-label">Via</span>
            <div className="segmented" role="group" aria-label="Route">
              {PROVIDERS.map(([id, name, title]) => (
                <button
                  key={id}
                  type="button"
                  className={id === settings.provider ? "seg is-on" : "seg"}
                  onClick={() => pickProvider(id)}
                  title={title}
                  aria-pressed={id === settings.provider}
                >
                  {name}
                </button>
              ))}
            </div>
          </>
        )}
      </div>

      {askingNew && (
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

      <div className={compact ? "chat-thread is-compact" : "chat-thread"} ref={threadRef}>
        {showKeyForm ? (
          <ApiKeyPrompt
            hint={settings?.key_hint ?? null}
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
              {entry.lines.map((line, i) => (
                <span className="msg-line" key={i}>
                  {line}
                </span>
              ))}
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
        {!showKeyForm && entries.length === 0 && suggestions.length > 0 && (
          <div className="chat-suggestions">
            {suggestions.map((text) => (
              <button
                key={text}
                type="button"
                className="chat-suggestion"
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
            placeholder={showKeyForm ? "Add an API key to start" : "Ask about the board…"}
            value={draft}
            disabled={showKeyForm || sending}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void send(draft);
            }}
            aria-label="Ask Claude"
          />
          <button
            type="button"
            className="btn-primary"
            disabled={showKeyForm || sending || draft.trim() === ""}
            onClick={() => send(draft)}
          >
            Send
          </button>
        </div>
        <span className="muted chat-foot">
          {model} · {note?.[1] ?? activeEffort} ·{" "}
          {settings?.provider === "claude_code" ? "via Claude Code" : "via the API"} · reads your
          board, never writes to Sleeper
          {settings?.has_key === true && (
            <>
              {" · "}
              <button type="button" className="link-btn" onClick={() => setShowKeyForm((s) => !s)}>
                {showKeyForm ? "cancel" : "change key"}
              </button>
            </>
          )}
        </span>
      </div>
    </aside>
  );
}
