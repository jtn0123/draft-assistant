// The last line of defence around a screen that fails to render.
//
// Suspense handles a chunk that is still loading; it does nothing for one that
// arrives broken or never arrives at all. Without a boundary React responds to
// an uncaught render error by unmounting the entire tree, so one bad chunk —
// a half-finished install, a corrupted download — takes the whole window with
// it and leaves nothing on screen to explain why.

import { Component, type ErrorInfo, type ReactNode } from "react";

interface State {
  failed: boolean;
}

export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { failed: false };

  static getDerivedStateFromError(): State {
    return { failed: true };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    // Nothing here can recover on its own, but the console is where anyone
    // debugging this will look first.
    console.error("A screen could not be shown", error, info.componentStack);
  }

  render() {
    if (!this.state.failed) return this.props.children;
    return (
      <div className="season-loading is-error">
        <span>This part of the app could not be shown. Reloading usually fixes it.</span>
        <button type="button" className="btn-primary" onClick={() => window.location.reload()}>
          Reload
        </button>
      </div>
    );
  }
}
