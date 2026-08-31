// Modal confirm and the toast strip.

import { useEffect, useRef } from "react";
import { useFocusTrap } from "./useFocusTrap";

export function ConfirmDialog({
  pickLabel,
  playerName,
  onConfirm,
  onCancel,
}: {
  pickLabel: string;
  playerName: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const dialog = useRef<HTMLDivElement>(null);
  const confirmRef = useRef<HTMLButtonElement>(null);
  // Whatever was focused when the dialog opened — a board row, usually.
  const opener = useRef<HTMLElement | null>(null);

  useEffect(() => {
    opener.current = document.activeElement as HTMLElement | null;
    confirmRef.current?.focus();
    return () => {
      // Back to the row they were on, not the top of a two-hundred-row board.
      opener.current?.focus();
      opener.current = null;
    };
  }, []);

  // Escape, Tab containment, and the board behind held inert — the same trap
  // the zoomed-picture overlay uses, so there is only one of them to get right.
  useFocusTrap(dialog, onCancel);

  return (
    <div className="scrim" onClick={onCancel} role="presentation">
      <div
        className="dialog"
        ref={dialog}
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-labelledby="confirm-title"
      >
        <span className="eyebrow">{pickLabel}</span>
        <span className="dialog-title" id="confirm-title">
          Mark {playerName} as drafted?
        </span>
        <span className="mid dialog-note">
          This records the pick locally. It does not draft them in Sleeper, and live sync from
          Sleeper overrides it.
        </span>
        <div className="dialog-actions">
          <button type="button" className="btn-primary" onClick={onConfirm} ref={confirmRef}>
            Mark drafted
          </button>
          <button type="button" className="btn-ghost" onClick={onCancel}>
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}

export function Toast({ message, onDismiss }: { message: string; onDismiss: () => void }) {
  return (
    <div className="toast" role="status">
      <span>{message}</span>
      <button type="button" className="link-btn" onClick={onDismiss}>
        Dismiss
      </button>
    </div>
  );
}
