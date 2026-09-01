// Replay: what the browser preview reads, and how it keeps reading it.
//
// Outside Tauri the app is served a captured state dump instead of a live
// engine (see `api.ts`). By default that is the checked-in fixture, read once.
// Point `?replay=<url>` at a file some other process keeps rewriting —
// `scripts/replay-sleeper.mjs` does exactly that — and the same source is
// re-read on a timer, with every newer dump pushed through the same listeners
// the desktop poller feeds. The screens cannot tell the difference.

import type { PollHealth } from "./types";

/** How often a replay source is re-read. Matches the desktop draft poller. */
export const REPLAY_POLL_MS = 3000;

/** Where a preview reads one kind of dump from, and whether it moves. */
export interface ReplaySource {
  /** The URL to fetch. */
  url: string;
  /** True when the URL came from `?replay=`, so it is worth re-reading. */
  live: boolean;
}

/**
 * Resolve one feed's source: the query parameter if it is there and not blank,
 * otherwise the checked-in fixture, read once and never polled.
 */
export function replaySource(search: string, parameter: string, fixture: string): ReplaySource {
  const given = new URLSearchParams(search).get(parameter)?.trim();
  return given === undefined || given === ""
    ? { url: fixture, live: false }
    : { url: given, live: true };
}

/** What a feed needs to know about the shape it is carrying. */
export interface FeedSpec<V> {
  source: ReplaySource;
  /** Said when the default fixture is missing — it names the file to add. */
  missing: string;
  /** "draft state" / "season scores": what the source was supposed to hold. */
  what: string;
  validate: (value: V) => V;
  generatedAt: (view: V) => number;
}

/**
 * Read one dump. Every failure here is a wrong path or a server that is not
 * writing, so each one says which — a preview that renders nothing is
 * otherwise indistinguishable from a preview that rendered an empty league.
 */
export async function readDump<V>(spec: FeedSpec<V>): Promise<V> {
  const { url, live } = spec.source;
  const response = await fetch(url, { cache: "no-store" });
  if (!response.ok) {
    throw new Error(live ? `replay source ${url} returned ${response.status}` : spec.missing);
  }
  // A path the dev server does not know answers 200 with index.html, so a
  // parse failure is the common case rather than the exotic one. Its own
  // message ("Unexpected token '<'") means nothing to whoever is looking at
  // the screen.
  let body: unknown;
  try {
    body = (await response.json()) as unknown;
  } catch {
    throw new Error(
      `could not read ${spec.what} from ${url} — it is not a state dump ` +
        `(check the path, and that the replay server is writing it)`,
    );
  }
  return spec.validate(body as V);
}

/**
 * One replayed feed: the cached dump the screens read on demand, plus the
 * timer that re-reads a live source and pushes what is newer.
 */
export class ReplayFeed<V> {
  private cached: V | null = null;
  private lastGeneratedAt = Number.NEGATIVE_INFINITY;
  private timer: ReturnType<typeof setInterval> | undefined;
  private readonly views: ((view: V) => void)[] = [];
  private readonly healths: ((health: PollHealth) => void)[] = [];

  constructor(private readonly spec: FeedSpec<V>) {}

  /** True when a `?replay=` source was given, so polling means something. */
  get live(): boolean {
    return this.spec.source.live;
  }

  /** The dump as the screens last saw it, read on the first call. */
  async current(): Promise<V> {
    if (this.cached === null) await this.reload();
    // `reload` always assigns; the cast keeps the null out of the signature.
    return this.cached as V;
  }

  /** Re-read the source now, whatever was cached. */
  async reload(): Promise<V> {
    const view = await readDump(this.spec);
    this.cached = view;
    this.lastGeneratedAt = this.spec.generatedAt(view);
    return view;
  }

  /**
   * A refresh the preview can honestly do: re-read a moving source, or hand
   * back the fixture that is never going to change.
   */
  refresh(): Promise<V> {
    return this.live ? this.reload() : this.current();
  }

  start(intervalMs: number = REPLAY_POLL_MS): void {
    this.stop();
    this.timer = setInterval(() => void this.poll(), intervalMs);
  }

  stop(): void {
    if (this.timer !== undefined) clearInterval(this.timer);
    this.timer = undefined;
  }

  /**
   * One tick. Each `dump_state` run restarts its own sequence numbering, so
   * `generated_at` is what orders the dumps.
   */
  async poll(): Promise<void> {
    try {
      const next = await readDump(this.spec);
      const at = this.spec.generatedAt(next);
      if (at <= this.lastGeneratedAt) return;
      this.lastGeneratedAt = at;
      this.cached = next;
      for (const handler of [...this.views]) handler(next);
      const health: PollHealth = {
        last_success_at: at,
        consecutive_failures: 0,
        last_error: null,
      };
      for (const handler of [...this.healths]) handler(health);
    } catch {
      // A dump caught half-written parses badly for a moment. The next tick
      // reads it whole; saying so would only flap the health badge.
    }
  }

  onView(handler: (view: V) => void): () => void {
    return subscribe(this.views, handler);
  }

  onHealth(handler: (health: PollHealth) => void): () => void {
    return subscribe(this.healths, handler);
  }
}

/** Add a handler and hand back the one call that removes it again. */
function subscribe<H>(handlers: H[], handler: H): () => void {
  handlers.push(handler);
  return () => {
    const at = handlers.indexOf(handler);
    if (at >= 0) handlers.splice(at, 1);
  };
}
