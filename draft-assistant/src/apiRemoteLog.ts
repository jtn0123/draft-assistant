// Where a follower's own failures go.
//
// The host's log is the host's: reporting into it over the wire would let any
// paired device write lines the host cannot account for, so the follower never
// does. But a follower that crashed used to leave no line anywhere, because
// its `logFrontendError` was a no-op. A follower is still the desktop app,
// with a Rust backend and a log file of its own under it, so inside Tauri the
// report goes there. In a plain browser tab there is no file: the line goes
// to the console and into a ring buffer the diagnostics dialog can show.

import { invoke } from "@tauri-apps/api/core";
import { getHostSync } from "./hostSync";
import type { Diagnostics, DraftView } from "./types";

/** How many lines the browser keeps. Enough to see what led up to a failure;
 *  small enough that a component failing on every poll cannot grow it. */
const RING_SIZE = 50;

const ring: string[] = [];

/** True when this window is the desktop app, follower or not. */
function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** One line in the shape the backend writes, so the tail reads the same
 *  whichever of the two places it came from. */
function line(message: string, source: string, stack?: string): string {
  const head = `${new Date().toISOString()} ERROR frontend: ${message} where=${source}`;
  return stack === undefined ? head : `${head}\n${stack}`;
}

/** The follower's `logFrontendError`: the local backend's log inside Tauri,
 *  the console and the ring buffer anywhere else. */
export function followerLog(message: string, source: string, stack?: string): Promise<void> {
  if (inTauri()) return invoke<void>("log_frontend_error", { message, source, stack });
  const text = line(message, source, stack);
  ring.push(text);
  if (ring.length > RING_SIZE) ring.shift();
  console.error(text);
  return Promise.resolve();
}

/** The log half of a follower's diagnostics: the local file when there is
 *  one, the ring buffer when there is not. */
export async function followerLogDiagnostics(): Promise<
  Pick<Diagnostics, "app_version" | "log_path" | "log_level" | "log_tail">
> {
  if (inTauri()) {
    try {
      const local = await invoke<Diagnostics>("diagnostics");
      return {
        app_version: local.app_version,
        log_path: local.log_path,
        log_level: local.log_level,
        log_tail: local.log_tail,
      };
    } catch {
      // The local backend did not answer; what this window kept is below.
    }
  }
  return { app_version: "", log_path: null, log_level: "info", log_tail: [...ring] };
}

/**
 * The whole of a follower's diagnostics report.
 *
 * The host's log is the host's: reading it over the wire would hand every
 * paired device the host's error history. What is reported is what this
 * window knows, and only that. It used to claim a poller it does not have,
 * saying `polling: true` for any view at all with no poll record beside it,
 * which rendered as "Live sync: On, nothing reported yet" on the one screen
 * meant to answer "what happened?". The truth is the host's own account of
 * its sync, off the heartbeat, and the poll record carried in the last view
 * the host sent.
 */
export async function followerDiagnostics(
  hostName: string,
  view: DraftView | null,
): Promise<Diagnostics> {
  return {
    ...(await followerLogDiagnostics()),
    platform: `following ${hostName}`,
    league_id: view?.league.league_id ?? null,
    league_name: view?.league.name ?? null,
    draft_id: null,
    platform_name: view?.league.platform ?? null,
    polling: getHostSync().polling === true,
    poll:
      view === null
        ? null
        : {
            last_success_at: view.data_health.poll_last_success_at ?? view.generated_at,
            consecutive_failures: view.data_health.poll_consecutive_failures,
            last_error: view.data_health.poll_last_error,
          },
    companion_enabled: false,
    companion_devices: 0,
  };
}

/** Open the follower's own log folder, or explain why there is none. */
export function followerOpenLogFolder(): Promise<string> {
  if (inTauri()) return invoke<string>("open_log_folder");
  return Promise.reject(new Error("A browser tab keeps no log file; the lines are in the console"));
}

/** Set the follower's own log level; a browser tab has only one. */
export function followerSetLogLevel(level: string): Promise<string> {
  if (inTauri()) return invoke<string>("set_log_level", { level });
  return Promise.reject(new Error("A browser tab has no log level to set"));
}

/** Test-only: forget the lines kept in the browser. */
export function resetFollowerLog(): void {
  ring.length = 0;
}
