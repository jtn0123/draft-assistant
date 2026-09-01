// Adding or replacing the Anthropic API key, from inside the chat thread.
// Split out of `Chat.tsx`, which has the panel's own work to do.

import { useState } from "react";
import { api } from "../api";

export function ChatKeyForm({ hint, onSaved }: { hint: string | null; onSaved: () => void }) {
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
      <button
        type="button"
        className="btn-primary"
        disabled={!key.trim() || busy}
        onClick={() => void save()}
      >
        {busy ? "Saving…" : "Save key"}
      </button>
      {error && <div className="error">{error}</div>}
    </div>
  );
}
