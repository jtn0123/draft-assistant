// The last line of defence around a screen that fails to render.
//
// Suspense handles a chunk that is still loading; it does nothing for one that
// arrives broken or never arrives at all. Without a boundary React responds to
// an uncaught render error by unmounting the entire tree, so one bad chunk —
// a half-finished install, a corrupted download — takes the whole window with
// it and leaves nothing on screen to explain why.
//
// The fallback also has to be the place the failure is recorded. `console.error`
// on its own was invisible in a shipped app: nobody opens devtools in a
// WKWebView, so a screen that would not render left no trace anywhere.

import { Component, type ErrorInfo, type ReactNode } from "react";
import { reportError } from "../errorReport";

interface State {
  failed: boolean;
  /** What to put on the clipboard: the error and where in the tree it came
   *  from. Held so "Copy details" has something to copy after the render that
   *  failed is long gone. */
  details: string;
}

export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { failed: false, details: "" };

  static getDerivedStateFromError(error: Error): State {
    return { failed: true, details: `${error.name}: ${error.message}` };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    const stack = (info.componentStack ?? "").trim();
    this.setState({
      details: `${error.name}: ${error.message}\n${error.stack ?? ""}\n${stack}`.trim(),
    });
    // The console is where anyone with devtools open looks first; the log is
    // where everyone else's copy of this ends up.
    console.error("A screen could not be shown", error, info.componentStack);
    reportError(`${error.name}: ${error.message}`, "render");
  }

  render() {
    if (!this.state.failed) return this.props.children;
    return (
      <div className="season-loading is-error">
        <span>This part of the app could not be shown. Reloading usually fixes it.</span>
        <button type="button" className="btn-primary" onClick={() => window.location.reload()}>
          Reload
        </button>
        <button
          type="button"
          className="btn-ghost"
          onClick={() => void navigator.clipboard?.writeText(this.state.details)}
        >
          Copy details
        </button>
      </div>
    );
  }
}
