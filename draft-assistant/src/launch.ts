/**
 * Restoring the saved league at launch is the one request the user cannot
 * retry with a click: a stalled connect there dropped them on an empty setup
 * form. A network failure is tried again briefly; anything else (a bad id,
 * a rejected payload) is reported at once.
 */

/** Delays before the second and third attempts. */
export const LAUNCH_RETRY_DELAYS_MS = [1000, 3000];

/** The client's connect/transfer timeouts and DNS or socket failures. */
export function transientNetworkError(message: string): boolean {
  return /timed out|error sending request|connect|network|dns|reset/i.test(message);
}

export async function withRetry<T>(
  run: () => Promise<T>,
  delays: number[],
  retryIf: (message: string) => boolean,
): Promise<T> {
  let attempt = 0;
  for (;;) {
    try {
      return await run();
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      if (attempt >= delays.length || !retryIf(message)) throw e;
      await new Promise((resolve) => setTimeout(resolve, delays[attempt]));
      attempt += 1;
    }
  }
}
