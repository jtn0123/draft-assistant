// Modal confirm and the toast strip.

import { useEffect, useRef } from "react";
import { platformName } from "../leagues";
import type { Platform } from "../types";
import { useFocusTrap } from "./useFocusTrap";

export function ConfirmDialog({
  pickLabel,
  playerName,
  platform,
  onConfirm,
  onCancel,
}: {
  pickLabel: string;
  playerName: string;
  /** Which service the league is read from. The sentence below names it, and
   *  it said "Sleeper" to Yahoo players until this was handed in. */
  platform: Platform;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const service = platformName(platform);
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
    <div
      className="scrim"
      role="presentation"
      // Only a click on the scrim itself closes; a click that started inside
      // the dialog just lands here on its way up. Checking the target beats
      // stopping propagation, which would have put a mouse-only handler on the
      // dialog with no keyboard equivalent.
      onClick={(e) => {
        if (e.target === e.currentTarget) onCancel();
      }}
    >
      <div
        className="dialog"
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="confirm-title"
      >
        <span className="eyebrow">{pickLabel}</span>
        <span className="dialog-title" id="confirm-title">
          Mark {playerName} as drafted?
        </span>
        <span className="mid dialog-note">
          This records the pick locally. It does not draft them in {service}, and live sync from{" "}
          {service} overrides it.
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

/** Something the user can do about the message — a retry, in practice. */
export interface ToastAction {
  label: string;
  onClick: () => void;
}

/**
 * The message strip under the header.
 *
 * A toast with an action is a failure the user has to decide about, so it is
 * announced straight away and waits to be answered; App leaves the five-second
 * timer off for those. A plain one is news, announced politely and gone on its
 * own. Either way the text is written once and only re-announced when it
 * changes, so an error sitting on screen is not read out over and over.
 */
export function Toast({
  message,
  action,
  onDismiss,
}: {
  message: string;
  action?: ToastAction;
  onDismiss: () => void;
}) {
  return (
    <div className="toast" role={action === undefined ? "status" : "alert"}>
      <span>{message}</span>
      {action !== undefined && (
        <button
          type="button"
          className="link-btn"
          onClick={() => {
            // Clear the failed attempt first: whatever happens next says so
            // itself, and a stale error under a fresh one reads badly.
            onDismiss();
            action.onClick();
          }}
        >
          {action.label}
        </button>
      )}
      <button type="button" className="link-btn" onClick={onDismiss}>
        Dismiss
      </button>
    </div>
  );
}
