// The one focus trap in the app, shared by everything that calls itself modal.

import { useEffect, type RefObject } from "react";

/** Everything inside a dialog the keyboard can land on. */
const STOPS = "button, [href], input, select, textarea, [tabindex]:not([tabindex='-1'])";

/**
 * Keep Tab inside `container`, close on Escape, and take the page behind out
 * of the keyboard's reach while it is open.
 *
 * A dialog that claims `aria-modal` and then lets Tab walk out into the page
 * behind it is worse than one that never claimed to be modal: the scrim stops
 * the mouse, so the focus ring lands on rows the user can neither see nor
 * click, with no way back but Tab-ing the whole way round.
 *
 * @param container the dialog box itself — the trap's boundary
 * @param onEscape  what Escape should do, usually closing the dialog
 * @param active    false while the dialog is not on screen
 */
export function useFocusTrap(
  container: RefObject<HTMLElement | null>,
  onEscape: () => void,
  active = true,
): void {
  useEffect(() => {
    if (!active) return undefined;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onEscape();
        return;
      }
      if (e.key !== "Tab") return;
      const stops = container.current?.querySelectorAll<HTMLElement>(STOPS);
      if (stops === undefined || stops.length === 0) return;
      const first = stops[0];
      const last = stops[stops.length - 1];
      if (first === undefined || last === undefined) return;
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [container, onEscape, active]);

  // Belt and braces for the trap above: while the overlay is up, everything
  // behind it is inert, so nothing back there can be tabbed to, clicked or
  // read out even if a stray control slips past the key handler.
  //
  // This used to look for `.shell`, which only the running app renders. The
  // two dialogs a brand new install sees first — "Join another Draft
  // Assistant" and "Connect Yahoo Fantasy" — open over the setup screen, which
  // has no shell, so the trap silently did nothing on the one screen where
  // there is nothing else to tab to. Walking up from the dialog and inerting
  // what sits beside it at each level works against whatever root is there.
  useEffect(() => {
    if (!active) return undefined;
    const dialog = container.current;
    if (dialog === null) return undefined;
    const root = document.getElementById("root") ?? document.body;
    const inerted: HTMLElement[] = [];
    let node: HTMLElement | null = dialog;
    while (node !== null && node !== root && node !== document.body) {
      for (const sibling of Array.from(node.parentElement?.children ?? [])) {
        // Already inert means an outer dialog put it that way and is going to
        // want it back; taking it off here would undo that overlay's trap.
        if (sibling === node || !(sibling instanceof HTMLElement)) continue;
        if (sibling.hasAttribute("inert")) continue;
        sibling.setAttribute("inert", "");
        inerted.push(sibling);
      }
      node = node.parentElement;
    }
    return () => {
      for (const element of inerted) element.removeAttribute("inert");
    };
  }, [container, active]);
}
