// Modal confirm and the toast strip.

import { useEffect, useRef } from "react";

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
  const confirmRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    confirmRef.current?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onCancel]);

  return (
    <div className="scrim" onClick={onCancel} role="presentation">
      <div
        className="dialog"
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
          This records the pick locally. It does not draft them in Sleeper, and live sync
          from Sleeper overrides it.
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
