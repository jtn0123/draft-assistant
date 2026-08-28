import { useState } from "react";
import type { StoredLeague } from "../types";

/**
 * Switch between leagues you have loaded, and add another by ID.
 *
 * Every league you load is remembered in the app config; until now nothing
 * read that list back, so loading a mock draft to practise on was a one-way
 * door. Switching is just `add_league` on an ID the backend already knows,
 * so it comes back from cache in about a second.
 */
export function LeaguePicker({
  leagues,
  activeId,
  disabled,
  onSwitch,
}: {
  leagues: StoredLeague[];
  activeId: string | null;
  disabled: boolean;
  onSwitch: (leagueId: string) => void;
}) {
  const [adding, setAdding] = useState(false);
  const [draft, setDraft] = useState("");

  const submit = () => {
    const id = draft.trim();
    if (!id) return;
    setDraft("");
    setAdding(false);
    onSwitch(id);
  };

  if (adding) {
    return (
      <div className="league-picker">
        <input
          className="league-add"
          value={draft}
          autoFocus
          placeholder="League or draft ID / sleeper.com URL"
          aria-label="League or draft ID"
          disabled={disabled}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") submit();
            if (e.key === "Escape") setAdding(false);
          }}
        />
        <button className="ghost small" onClick={submit} disabled={disabled || !draft.trim()}>
          Load
        </button>
        <button className="ghost small" onClick={() => setAdding(false)}>
          Cancel
        </button>
      </div>
    );
  }

  return (
    <div className="league-picker">
      <select
        aria-label="League"
        value={activeId ?? ""}
        disabled={disabled}
        onChange={(e) => {
          if (e.target.value === "__add__") {
            setAdding(true);
            return;
          }
          if (e.target.value !== activeId) onSwitch(e.target.value);
        }}
      >
        {leagues.map((l) => (
          <option key={l.league_id} value={l.league_id}>
            {l.name} ({l.season})
          </option>
        ))}
        <option value="__add__">Add a league or draft…</option>
      </select>
    </div>
  );
}
