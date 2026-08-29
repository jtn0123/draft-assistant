import { useLayoutEffect, useRef } from "react";

/**
 * "Mark X as drafted?" — the one destructive action in the app, so it gets a
 * real modal: focus moves in, Escape cancels, and focus goes back to the
 * button that opened it when it closes.
 */
export function ConfirmDialog({
  name,
  pick,
  slot,
  onConfirm,
  onCancel,
}: {
  name: string;
  /** Already formatted — the dialog inherits whatever numbering the app is in. */
  pick: string;
  slot: number;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const confirmRef = useRef<HTMLButtonElement>(null);
  // Whatever had focus when this rendered — the row's Draft button. Captured
  // once, outside the effect, because dev-mode StrictMode re-runs the effect
  // after the dialog has already taken focus.
  const opener = useRef<HTMLElement | null>(null);
  if (opener.current === null) opener.current = document.activeElement as HTMLElement | null;
  const pendingRestore = useRef<number | undefined>(undefined);

  useLayoutEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    window.clearTimeout(pendingRestore.current);
    if (!dialog.open) dialog.showModal();
    confirmRef.current?.focus();
    return () => {
      // The parent unmounts us rather than calling close(), and the browser
      // drops focus to <body> when the focused button leaves the DOM. Put it
      // back on the opener once that has happened. Cancelled by a remount.
      pendingRestore.current = window.setTimeout(() => opener.current?.focus(), 0);
    };
  }, []);

  return (
    <dialog
      ref={ref}
      className="modal"
      aria-labelledby="confirm-title"
      onCancel={(e) => {
        e.preventDefault();
        onCancel();
      }}
      onKeyDown={(e) => {
        if (e.key === "Escape") onCancel();
      }}
      onClick={(e) => {
        // A click on the backdrop reaches the dialog element itself.
        if (e.target === e.currentTarget) onCancel();
      }}
    >
      <p id="confirm-title">
        Mark <strong>{name}</strong> as drafted at pick {pick} (slot {slot})?
      </p>
      <p className="muted small-text">
        Manual picks are a fallback — live sync from Sleeper overrides them.
      </p>
      <div className="modal-actions">
        <button ref={confirmRef} onClick={onConfirm}>
          Confirm
        </button>
        <button className="ghost" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </dialog>
  );
}
