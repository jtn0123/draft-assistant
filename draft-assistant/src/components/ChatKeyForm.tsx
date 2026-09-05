// Adding or replacing the Anthropic API key, from inside the chat thread.
// Split out of `Chat.tsx`, which has the panel's own work to do.

import { useState } from "react";
import { api } from "../api";
import { describeError } from "../errorText";

/** Where the key actually ends up, said plainly — "stored locally" is not the
 *  same promise as "in the Keychain", and the user is entitled to know which
 *  one they are getting. */
const STORE_NOTE: Record<string, string> = {
  keychain: "Kept in the macOS Keychain, under this app's own item.",
  file: "No Keychain on this machine — kept in a file in this app's data directory, readable by your user account.",
};

export function ChatKeyForm({
  hint,
  store,
  onSaved,
}: {
  hint: string | null;
  store: "keychain" | "file" | null;
  onSaved: () => void;
}) {
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
      setError(describeError(e));
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
          ? "Ask Claude sends your board to the Anthropic API. The key stays on this Mac and goes nowhere else."
          : `Currently using ${hint}.`}
      </span>
      {store !== null && <span className="muted small">{STORE_NOTE[store]}</span>}
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
      {error && (
        <div className="error" role="alert">
          {error}
        </div>
      )}
    </div>
  );
}
