// The Refresh control on the season header: asks the backend for this week's
// scoring again, says so while it waits, and says why when it cannot.
//
// The desktop had no caller for `refresh_season` at all: the command, its
// week-rollover check and its tests all existed, and the only way to pull the
// live slice by hand was to close the league and open it again.

import { useState } from "react";
import { api } from "../api";
import { problem } from "../format";
import type { SeasonView } from "../season-types";

export function SeasonRefresh({ onView }: { onView: (view: SeasonView) => void }) {
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  // The same shape as the draft screen's re-pull: refuse a second click while
  // one is out, hand the answer up, keep the reason on screen when it fails.
  const refresh = async () => {
    if (busy) return;
    setBusy(true);
    setFailure(null);
    try {
      onView(await api.refreshSeason());
    } catch (e) {
      setFailure(problem("Could not refresh the season", e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="season-refresh">
      <button
        type="button"
        className="btn-ghost btn-row"
        onClick={() => void refresh()}
        disabled={busy}
        title="Ask for this week's scoring again now"
      >
        {busy ? "Refreshing…" : "Refresh"}
      </button>
      {failure !== null && (
        <span role="alert" className="muted small season-stat-sub">
          {failure}
        </span>
      )}
    </div>
  );
}
