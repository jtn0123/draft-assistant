// Bring a panel that opened somewhere off screen into view.

import { useEffect, useRef } from "react";

/** Below this the shell drops to one column and the chat panel is stacked
 *  underneath the board rather than beside it. It has to match the breakpoint
 *  App.css and chat.css use, or the panel scrolls into view on a layout where
 *  it was already visible. */
export const STACKED_QUERY = "(max-width: 1100px)";

/**
 * Scroll the element this ref is put on into view when it first appears, but
 * only while the layout is the stacked one.
 *
 * At the app's minimum window width the chat column moves below the board,
 * which is a full screen further down than the button that opened it. Nothing
 * on screen changed when the panel opened, so the button read as broken and
 * people pressed it again — closing the panel they had just been given.
 *
 * Two things are deliberately soft. `matchMedia` is missing in some test
 * environments and older embedded webviews, and its absence means the wide
 * layout, where there is nothing to scroll to. `scrollIntoView` is missing in
 * jsdom, so the call is guarded rather than mocked into every test that
 * renders a chat panel.
 */
export function useRevealOnMount<T extends HTMLElement>(query: string = STACKED_QUERY) {
  const ref = useRef<T | null>(null);
  useEffect(() => {
    const node = ref.current;
    if (node === null) return;
    if (typeof window.matchMedia !== "function") return;
    if (!window.matchMedia(query).matches) return;
    if (typeof node.scrollIntoView !== "function") return;
    node.scrollIntoView({ behavior: "smooth", block: "start" });
  }, [query]);
  return ref;
}
