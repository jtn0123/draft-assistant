import { lazy } from "react";

// The app shows one screen at a time and many sessions never open the chat, so
// none of these belong in the bundle the window parses before first paint.
// Each `import()` below becomes its own chunk, fetched when it is first shown.
// They live here rather than in App.tsx only to keep that file under the
// 500-line cap.

export const DraftScreen = lazy(() =>
  import("./DraftScreen").then((m) => ({ default: m.DraftScreen })),
);

export const SeasonScreen = lazy(() =>
  import("./SeasonScreen").then((m) => ({ default: m.SeasonScreen })),
);

export const Chat = lazy(() => import("./Chat").then((m) => ({ default: m.Chat })));

// Shown while a screen's chunk is still in flight. Deliberately plain: the
// header is already painted above it, and a spinner for a fetch that normally
// finishes within a frame reads as a stall.
export function ScreenFallback() {
  return (
    <div className="muted" role="status">
      Loading…
    </div>
  );
}
