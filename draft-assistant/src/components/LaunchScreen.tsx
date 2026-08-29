import type { LaunchStatus } from "../launch";

/**
 * The launch, told as it happens: connecting, still trying (and why), or
 * unable to connect — with a retry and a way to a different league. No
 * status yet means the config itself is still being read.
 */
export function LaunchScreen({
  status,
  onRetry,
  onSetup,
}: {
  status: LaunchStatus | null;
  onRetry: () => void;
  onSetup: () => void;
}) {
  if (status?.failed) {
    return (
      <div className="setup">
        <h1>Unable to connect</h1>
        <p className="muted">
          Sleeper did not answer after {status.total} tries. The league is still saved; try
          again when the connection is back.
        </p>
        <div className="error" role="alert">
          {status.error}
        </div>
        <div className="launch-actions">
          <button onClick={onRetry}>Try again</button>
          <button className="ghost" onClick={onSetup}>
            Load a different league
          </button>
        </div>
      </div>
    );
  }
  const retrying = status !== null && status.attempt > 1;
  return (
    <div className="setup">
      <h1>Draft Assistant</h1>
      <div className="launch-status" role="status">
        <span className={retrying ? "launch-dot waiting" : "launch-dot"} aria-hidden="true" />
        <span>
          {status === null
            ? "Reading your settings…"
            : retrying
              ? `Sleeper isn't answering — trying again (attempt ${status.attempt} of ${status.total})…`
              : "Connecting to Sleeper and loading your league…"}
        </span>
        {retrying && status.error && <span className="launch-detail">{status.error}</span>}
      </div>
    </div>
  );
}
