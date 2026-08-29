import type { DraftView } from "../types";
import { PlayerName } from "./PlayerCard";

/** My roster as drafted, with keepers tagged and the open starting slots. */
export function RosterCard({ view }: { view: DraftView }) {
  const roster = view.my_roster;
  const starters = view.league.roster_positions.filter((s) => s !== "BN");
  const benchSize = view.league.roster_positions.filter((s) => s === "BN").length;
  return (
    <section>
      <h2>My roster</h2>
      {roster === null ? (
        <p className="muted">Set your Sleeper username to track your team.</p>
      ) : roster.players.length === 0 ? (
        <p className="muted">No picks yet.</p>
      ) : (
        <ul className="roster" aria-label="My roster">
          {roster.players.map((p) => (
            <li key={p.player_id}>
              <span className={`pos-badge pos-${p.position}`}>{p.position}</span>
              <span>
                <PlayerName id={p.player_id}>{p.name}</PlayerName>
                {p.is_keeper && (
                  <span className="keeper-tag" title="Kept from last season, not drafted tonight">
                    keeper
                  </span>
                )}
              </span>
              <span className="muted">R{p.round}</span>
            </li>
          ))}
        </ul>
      )}
      {roster !== null && roster.open_starters.length > 0 && (
        <p className="muted small-text">
          Open starters: {roster.open_starters.map(([slot, n]) => `${slot}×${n}`).join(", ")} ·{" "}
          {starters.length} starters + {benchSize} bench
        </p>
      )}
    </section>
  );
}
