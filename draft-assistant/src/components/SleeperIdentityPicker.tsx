import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { describeError } from "../errorText";
import type { DraftView, SleeperMember } from "../types";

export function SleeperIdentityPicker({
  view,
  onSaved,
  onError,
}: {
  view: DraftView;
  onSaved: (view: DraftView) => void;
  onError?: (message: string) => void;
}) {
  const [members, setMembers] = useState<SleeperMember[]>([]);
  const [username, setUsername] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const [revision, setRevision] = useState(0);
  const generation = useRef(0);
  useEffect(() => {
    const token = ++generation.current;
    void (async () => {
      await Promise.resolve();
      if (generation.current !== token) return;
      setMembers([]);
      setError(null);
      setSaved(null);
      setLoading(true);
      setSaving(false);
      setUsername("");
      try {
        const next = await api.listSleeperMembers();
        if (generation.current === token) setMembers(next);
      } catch (cause) {
        if (generation.current === token) setError(describeError(cause));
      } finally {
        if (generation.current === token) setLoading(false);
      }
    })();
    return () => {
      generation.current = token + 1;
    };
  }, [view.league.league_id, revision]);

  const save = async (value: string, label: string) => {
    const token = generation.current;
    setError(null);
    setSaved(null);
    setSaving(true);
    try {
      const id = await api.setMyUsername(value.trim());
      if (generation.current !== token) return;
      // getState rebuilds the current seat from the saved account, without a
      // full projection download or a second league load.
      const next = await api.getState();
      if (generation.current !== token || next.league.league_id !== view.league.league_id) return;
      setMembers((current) =>
        current.map((member) => ({ ...member, is_current: member.user_id === id })),
      );
      setSaved(
        `${label} is your default Sleeper identity. ${next.draft.seat_note ?? "Your team is now selected."}`,
      );
      onSaved(next);
    } catch (cause) {
      if (generation.current !== token) return;
      const message = describeError(cause);
      setError(message);
      onError?.(message);
    } finally {
      if (generation.current === token) setSaving(false);
    }
  };
  const selected = members.find((member) => member.is_current);
  return (
    <section id="sleeper-identity-picker" aria-labelledby="sleeper-identity-heading">
      <h3 id="sleeper-identity-heading">Your Sleeper identity</h3>
      <p>
        Choose yourself from {view.league.name}. This default is remembered on this Mac and used to
        find your team in Sleeper leagues.
      </p>
      {selected && (
        <p>
          Current default: <strong>{selected.display_name ?? selected.user_id}</strong>
        </p>
      )}
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (username.trim() && !saving) void save(username, username.trim());
        }}
      >
        <label htmlFor="sleeper-identity-username">Sleeper username</label>
        <input
          id="sleeper-identity-username"
          value={username}
          onChange={(event) => setUsername(event.target.value)}
          disabled={saving}
          autoComplete="off"
        />
        <button type="submit" disabled={saving || !username.trim()}>
          {saving ? "Saving identity..." : "Save username"}
        </button>
      </form>
      {loading && <p role="status">Loading league accounts...</p>}
      {!loading && !error && members.length === 0 && (
        <p>
          No league accounts were returned. For a mock draft or an unavailable member list, enter
          your Sleeper username above.
        </p>
      )}
      {members.length > 0 && (
        <ul aria-label="Sleeper league account names">
          {members.map((member) => (
            <li key={member.user_id}>
              <button
                type="button"
                disabled={saving || member.is_current}
                aria-pressed={member.is_current}
                onClick={() => void save(member.user_id, member.display_name ?? member.user_id)}
              >
                {member.display_name ?? `Account ${member.user_id}`}
                {member.is_current ? " (default)" : ""}
              </button>{" "}
              <small>
                {member.display_name ? `ID ${member.user_id}` : "Account name unavailable"} ·{" "}
                {member.draft_slot === null
                  ? "Draft seat pending"
                  : `Draft seat ${member.draft_slot}`}
              </small>
            </li>
          ))}
        </ul>
      )}
      {!loading && (
        <button type="button" disabled={saving} onClick={() => setRevision((value) => value + 1)}>
          Reload accounts
        </button>
      )}
      {view.draft.seat_note && <p>{view.draft.seat_note}</p>}
      {error && <p role="alert">{error}</p>}
      {saved && <p role="status">{saved}</p>}
    </section>
  );
}
