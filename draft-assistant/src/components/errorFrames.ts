// The part of a React component stack worth putting in a log line.

/** The first frames of React's component stack, one trimmed line each. Two
 *  is enough to name the screen and the component inside it; the whole stack
 *  is what the clipboard gets. */
export function firstFrames(componentStack: string | null | undefined, count = 2): string[] {
  return (componentStack ?? "")
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line !== "")
    .slice(0, count);
}
