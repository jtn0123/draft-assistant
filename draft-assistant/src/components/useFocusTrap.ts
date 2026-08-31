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

  // Belt and braces for the trap above: while the overlay is up, the app
  // behind it is inert, so nothing in there can be tabbed to, clicked or read
  // out even if a stray control slips past the key handler.
  useEffect(() => {
    if (!active) return undefined;
    const shell = document.querySelector<HTMLElement>(".shell");
    if (shell === null || shell.contains(container.current)) return undefined;
    shell.setAttribute("inert", "");
    return () => shell.removeAttribute("inert");
  }, [container, active]);
}
