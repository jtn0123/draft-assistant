// Settings -> "Diagnostics…": what to say when someone asks "what happened?"
//
// The failure this exists to prevent is the draft-night one. Something goes
// wrong, the toast is dismissed, and neither the user nor anyone reading over
// their shoulder can say which league was open, whether the poller was getting
// through, or where the log is. All of that was already known to the backend
// and none of it was reachable.
//
// Everything on this screen is safe to paste into a chat window: no pairing
// code, no token, no key, and a log tail the backend has already redacted.

import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { describeError } from "../errorText";
import type { Diagnostics as Report } from "../types";
import { diagnosticsText, pollSummary } from "./diagnosticsText";
import { useFocusTrap } from "./useFocusTrap";

import "../diagnostics.css";

/** One row of the fact table. */
function Fact({ name, value }: { name: string; value: string }) {
  return (
    <>
      <span className="diag-key">{name}</span>
      <span className="diag-value">{value}</span>
    </>
  );
}

export function Diagnostics({
  appVersion,
  onClose,
}: {
  /** What the shell believes it is running, used when the backend cannot say
   *  — which is every follower, since the version it would report is the
   *  host's rather than this window's. */
  appVersion: string;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDivElement>(null);
  const opener = useRef<HTMLElement | null>(null);
  const [report, setReport] = useState<Report | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    opener.current = document.activeElement as HTMLElement | null;
    return () => {
      opener.current?.focus();
      opener.current = null;
    };
  }, []);

  useFocusTrap(dialog, onClose);

  useEffect(() => {
    let cancelled = false;
    void api.diagnostics().then(
      (fresh) => {
        if (!cancelled) setReport(fresh);
      },
      (e: unknown) => {
        if (!cancelled) setError(describeError(e));
      },
    );
    return () => {
      cancelled = true;
    };
  }, []);

  const copy = (text: string, said: string) => {
    void navigator.clipboard
      ?.writeText(text)
      .then(() => setNote(said))
      .catch((e: unknown) => setError(describeError(e)));
  };

  const openFolder = () => {
    void api.openLogFolder().then(
      (path) => setNote(`Log folder: ${path}`),
      (e: unknown) => setError(describeError(e)),
    );
  };

  return (
    <div
      className="scrim"
      role="presentation"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="dialog"
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="diagnostics-title"
      >
        <span className="eyebrow">Settings</span>
        <span className="dialog-title" id="diagnostics-title">
          Diagnostics
        </span>
        <span className="mid dialog-note">
          Everything here is safe to share. No keys, no tokens, and no pairing code.
        </span>

        {report === null ? (
          <span className="diag-empty">{error ?? "Reading…"}</span>
        ) : (
          <>
            <div className="diag-facts">
              <Fact
                name="Version"
                value={report.app_version === "" ? appVersion : report.app_version}
              />
              <Fact name="Platform" value={report.platform} />
              <Fact
                name="League"
                value={
                  report.league_id === null
                    ? "None loaded"
                    : `${report.league_name ?? ""} · ${report.league_id}`
                }
              />
              <Fact name="Draft" value={report.draft_id ?? "None"} />
              <Fact name="Live sync" value={pollSummary(report)} />
              <Fact
                name="Phone"
                value={report.companion_enabled ? `On · ${report.companion_devices} paired` : "Off"}
              />
              <Fact name="Log file" value={report.log_path ?? "None on this machine"} />
            </div>

            {report.log_tail.length === 0 ? (
              <span className="diag-empty">Nothing in the log yet.</span>
            ) : (
              <pre className="diag-log">{report.log_tail.join("\n")}</pre>
            )}

            <div className="dialog-actions">
              <button
                type="button"
                className="btn-primary"
                onClick={() => copy(diagnosticsText(report, appVersion), "Copied")}
              >
                Copy diagnostics
              </button>
              {report.log_path !== null && (
                <button type="button" className="btn-ghost" onClick={openFolder}>
                  Open log folder
                </button>
              )}
              <button type="button" className="btn-ghost" onClick={onClose}>
                Close
              </button>
            </div>
            {note !== null && <span className="diag-empty">{note}</span>}
            {error !== null && <span className="diag-empty">{error}</span>}
          </>
        )}
      </div>
    </div>
  );
}
