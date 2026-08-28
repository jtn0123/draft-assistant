import { Component, type ErrorInfo, type ReactNode } from "react";

type Props = { children: ReactNode };
type State = { error: Error | null };

/**
 * Last line of defence for the live view. Without this, one exception during
 * render — a null in a field nobody expected, a bad fixture, a schema slip —
 * unmounts the whole tree to a blank window mid-draft.
 *
 * "Reload state" remounts <App/>, which re-reads config and re-pulls the
 * current draft state from the backend; the backend itself never stopped.
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("render failed", error, info.componentStack);
  }

  render() {
    const { error } = this.state;
    if (error === null) return this.props.children;
    return (
      <div className="setup" role="alert">
        <h1>Draft Assistant hit a display error</h1>
        <p className="error">{error.message || String(error)}</p>
        <p className="muted">
          The draft engine is still running and still polling Sleeper — only
          the screen failed. Reloading re-renders from the current state.
        </p>
        <div className="modal-actions">
          <button onClick={() => this.setState({ error: null })}>Reload state</button>
          <button className="ghost" onClick={() => window.location.reload()}>
            Restart app
          </button>
        </div>
      </div>
    );
  }
}
