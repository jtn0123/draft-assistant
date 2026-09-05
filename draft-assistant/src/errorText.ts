/**
 * The sentence to show for something that went wrong.
 *
 * `String(e)` on an Error gives "Error: the host is away", and the screens
 * that used it printed "Error: Error:" once the message had been built from
 * another error. Every surface reads the message and strips the prefix, so
 * that is written once here rather than five times with four spellings.
 */
export function describeError(e: unknown): string {
  return String(e instanceof Error ? e.message : e).replace(/^Error:\s*/, "");
}
