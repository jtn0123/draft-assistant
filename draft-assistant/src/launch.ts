/**
 * Restoring the saved league at launch is the one request the user cannot
 * retry with a click: a stalled connect there dropped them on an empty setup
 * form. A network failure is tried again briefly; anything else (a bad id,
 * a rejected payload) is reported at once.
 */

/** Delays before the second and third attempts. */
export const LAUNCH_RETRY_DELAYS_MS = [1000, 3000];

/** What the launch screen shows while the saved league is being restored. */
export interface LaunchStatus {
  leagueId: string;
  /** 1-based; `total` is the most it will try. */
  attempt: number;
  total: number;
  /** The last failure's message, once there has been one. */
  error: string | null;
  /** Every attempt failed; waiting on the user. */
  failed: boolean;
}

export function startingLaunch(leagueId: string): LaunchStatus {
  return { leagueId, attempt: 1, total: LAUNCH_RETRY_DELAYS_MS.length + 1, error: null, failed: false };
}

/** The client's connect/transfer timeouts and DNS or socket failures. */
export function transientNetworkError(message: string): boolean {
  return /timed out|error sending request|connect|network|dns|reset/i.test(message);
}

export async function withRetry<T>(
  run: () => Promise<T>,
  delays: number[],
  retryIf: (message: string) => boolean,
  /** Called before each further attempt with its 1-based number and why. */
  onRetry?: (attempt: number, lastError: string) => void,
): Promise<T> {
  let attempt = 0;
  for (;;) {
    try {
      return await run();
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      if (attempt >= delays.length || !retryIf(message)) throw e;
      onRetry?.(attempt + 2, message);
      await new Promise((resolve) => setTimeout(resolve, delays[attempt]));
      attempt += 1;
    }
  }
}
