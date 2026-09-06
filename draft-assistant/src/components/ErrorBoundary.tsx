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
import { describeError } from "../errorText";
import { firstFrames } from "./errorFrames";

interface State {
  failed: boolean;
  /** What to put on the clipboard: the error and where in the tree it came
   *  from. Held so "Copy details" has something to copy after the render that
   *  failed is long gone. */
  details: string;
  /** What "Copy details" last did: nothing yet, "Copied", or why it could
   *  not. The button used to fire and say nothing either way, so a paste
   *  that came up empty had no explanation. */
  copyNote: string | null;
}

export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { failed: false, details: "", copyNote: null };

  static getDerivedStateFromError(error: Error): State {
    return { failed: true, details: `${error.name}: ${error.message}`, copyNote: null };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    const stack = (info.componentStack ?? "").trim();
    this.setState({
      details: `${error.name}: ${error.message}\n${error.stack ?? ""}\n${stack}`.trim(),
    });
    // The console is where anyone with devtools open looks first; the log is
    // where everyone else's copy of this ends up.
    console.error("A screen could not be shown", error, info.componentStack);
    // The log line names where in the tree it happened, not only what was
    // thrown: "TypeError: x is undefined" on its own named no screen.
    const frames = firstFrames(info.componentStack);
    const where = frames.length === 0 ? "" : ` (${frames.join(" < ")})`;
    reportError(`${error.name}: ${error.message}${where}`, "render");
  }

  private copy = async () => {
    // Absent in a plain http page and in some webviews; the old `?.` made the
    // button do nothing there, which reads as a broken button on top of a
    // broken screen.
    const clipboard = navigator.clipboard as Clipboard | undefined;
    if (clipboard === undefined) {
      this.setState({ copyNote: "Copying is not available here" });
      return;
    }
    try {
      await clipboard.writeText(this.state.details);
      this.setState({ copyNote: "Copied" });
    } catch (e) {
      this.setState({ copyNote: `Could not copy: ${describeError(e)}` });
    }
  };

  render() {
    if (!this.state.failed) return this.props.children;
    return (
      // Announced: the screen that was here is simply gone, and nothing else
      // on the page changes to say so.
      <div className="season-loading is-error" role="alert">
        <span>This part of the app could not be shown. Reloading usually fixes it.</span>
        <button type="button" className="btn-primary" onClick={() => window.location.reload()}>
          Reload
        </button>
        <button type="button" className="btn-ghost" onClick={() => void this.copy()}>
          Copy details
        </button>
        {this.state.copyNote !== null && (
          <span className="muted small" role="status">
            {this.state.copyNote}
          </span>
        )}
      </div>
    );
  }
}
