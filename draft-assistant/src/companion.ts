// The companion feature's frontend state: which host this copy of the app
// follows (if any), and the host-side status the settings dialog drives.
//
// The follow record is the one thing that has to be readable before anything
// else in the app starts, because `api.ts` picks its backend off it. So the
// reading of it is a plain hoisted function over `localStorage` with no
// module-level state of its own — `api.ts` and this file import each other,
// and only a function declaration is safe to call across that cycle.

import { useCallback, useEffect, useState } from "react";
import { api } from "./api";
import { describeError } from "./errorText";
// The follower's connection state lives in its own module so `apiRemote` can
// set it without pulling in this file's dependency on `api`, and it is
// re-exported here because this is where the rest of the follow state is read.
export { useFollowStatus, followStatusMessage, type FollowStatus } from "./followStatus";
import type { CompanionDevice, CompanionStatus, DeviceKind, FollowRecord } from "./types";

const FOLLOW_KEY = "da.companion.follow";

/** The port the host listens on unless it says otherwise. */
export const DEFAULT_COMPANION_PORT = 7878;

function isFollowRecord(value: unknown): value is FollowRecord {
  if (typeof value !== "object" || value === null) return false;
  const record = value as Partial<FollowRecord>;
  return (
    typeof record.url === "string" &&
    record.url !== "" &&
    typeof record.token === "string" &&
    record.token !== "" &&
    typeof record.host_name === "string"
  );
}

/**
 * The host this app is following, or null for ordinary local mode.
 *
 * Guarded end to end: a browser that refuses storage, a half-written record,
 * or anything that is not the shape we wrote all read as "not following",
 * because the alternative is an app that will not start.
 */
export function readFollow(): FollowRecord | null {
  try {
    const raw = localStorage.getItem(FOLLOW_KEY);
    if (raw === null) return null;
    const parsed: unknown = JSON.parse(raw);
    return isFollowRecord(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

export function saveFollow(record: FollowRecord): void {
  try {
    localStorage.setItem(FOLLOW_KEY, JSON.stringify(record));
  } catch {
    // Nothing to do: the join simply will not survive a restart.
  }
}

export function clearFollow(): void {
  try {
    localStorage.removeItem(FOLLOW_KEY);
  } catch {
    // Already gone as far as this session is concerned.
  }
}

/** True when this window is a follower of someone else's app. */
export function isFollower(): boolean {
  return readFollow() !== null;
}

/**
 * The origin of whatever the user typed or pasted.
 *
 * Accepts `192.168.1.5:7878`, a bare `192.168.1.5` (the default port is
 * assumed), `http://192.168.1.5:7878/` as the QR encodes it, and the same
 * with a path or query on the end. Returns null when there is no host in it.
 */
export function parseHostAddress(typed: string): string | null {
  const trimmed = typed.trim();
  if (trimmed === "") return null;
  const withScheme = /^https?:\/\//i.test(trimmed) ? trimmed : `http://${trimmed}`;
  let url: URL;
  try {
    url = new URL(withScheme);
  } catch {
    return null;
  }
  if (url.hostname === "") return null;
  const port = url.port === "" ? String(DEFAULT_COMPANION_PORT) : url.port;
  return `${url.protocol}//${url.hostname}:${port}`;
}

/** Errors arrive as strings or Errors; this is the sentence to show. */
export function reason(e: unknown): string {
  return describeError(e);
}

/** What a failed pair says, per status code. */
function pairProblem(status: number): string {
  if (status === 403) return "That code is not the one on the host's screen.";
  if (status === 429) return "Too many tries. Wait a minute and start again.";
  return `The host answered ${status}.`;
}

/**
 * Swap a six-digit code for a token. `origin` comes from `parseHostAddress`.
 */
export async function pairWithHost(
  origin: string,
  code: string,
  deviceName: string,
  kind: DeviceKind = "desktop",
): Promise<FollowRecord> {
  // The failure is handled as a value rather than caught: a fetch that cannot
  // connect says only "Failed to fetch", and the useful half of the sentence
  // is the address we were trying, which is already here.
  const response = await fetch(`${origin}/api/pair`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ code: code.trim(), device_name: deviceName.trim(), kind }),
  }).then(
    (answer) => answer,
    () => null,
  );
  if (response === null) {
    throw new Error(
      `Could not reach ${origin}. Check the address and that both are on the same Wi-Fi.`,
    );
  }
  if (!response.ok) throw new Error(pairProblem(response.status));
  const body = (await response.json()) as { token?: string; host_name?: string };
  if (typeof body.token !== "string" || body.token === "") {
    throw new Error("The host paired but sent no token.");
  }
  return { url: origin, token: body.token, host_name: body.host_name ?? "the host" };
}

/**
 * Whether the companion server is up, for the settings row that says so.
 *
 * Asked once at launch and again whenever `token` changes — the dialog hands
 * its own open/closed state in, so the row is right the moment it is closed.
 * A follower never asks: the call is the host's and would only be refused.
 */
export function useCompanionEnabled(ask: boolean, token: unknown): boolean {
  const [on, setOn] = useState(false);
  useEffect(() => {
    if (!ask) return undefined;
    let cancelled = false;
    api
      .companionStatus()
      .then((status) => {
        if (!cancelled) setOn(status.enabled);
      })
      .catch(() => {
        // Nothing is being served, as far as the menu is concerned.
      });
    return () => {
      cancelled = true;
    };
  }, [ask, token]);
  return on;
}

/** What the host panel reads and drives. */
export interface Companion {
  status: CompanionStatus | null;
  devices: CompanionDevice[];
  busy: boolean;
  error: string | null;
  enable: () => void;
  disable: () => void;
  revoke: () => void;
  rename: (name: string) => void;
}

/**
 * The host half: the server's status, the device list kept current by the
 * `companion-devices` event, and the four things the dialog can do to it.
 */
export function useCompanion(): Companion {
  const [status, setStatus] = useState<CompanionStatus | null>(null);
  const [devices, setDevices] = useState<CompanionDevice[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const take = useCallback((next: CompanionStatus) => {
    setStatus(next);
    setDevices(next.devices);
  }, []);

  useEffect(() => {
    let cancelled = false;
    api
      .companionStatus()
      .then((next) => {
        if (!cancelled) take(next);
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(reason(e));
      });
    return () => {
      cancelled = true;
    };
  }, [take]);

  // The list goes stale the moment a phone connects, so it is pushed rather
  // than polled. The same event fires when the pairing code rotates (after a
  // pairing, or on the idle timer), and the code only lives in the status, so
  // each push also re-reads the status: otherwise the dialog keeps showing a
  // code the host has already retired. Unsubscribing is awaited through the
  // same promise it arrived on, so a dialog closed before the listener landed
  // still detaches.
  useEffect(() => {
    let live = true;
    const pending = api.onCompanionDevices((next) => {
      if (!live) return;
      setDevices(next);
      api
        .companionStatus()
        .then((fresh) => {
          if (live) setStatus(fresh);
        })
        .catch(() => undefined);
    });
    return () => {
      live = false;
      void pending.then((off) => off());
    };
  }, []);

  const run = useCallback(
    (what: () => Promise<CompanionStatus>) => {
      setBusy(true);
      setError(null);
      what()
        .then(take)
        .catch((e: unknown) => setError(reason(e)))
        .finally(() => setBusy(false));
    },
    [take],
  );

  const rename = useCallback(
    (name: string) => {
      setError(null);
      api
        .setDeviceName(name)
        .then((kept) => setStatus((prev) => (prev === null ? prev : { ...prev, host_name: kept })))
        .catch((e: unknown) => setError(reason(e)));
    },
    [setStatus],
  );

  return {
    status,
    devices,
    busy,
    error,
    enable: () => run(() => api.companionEnable()),
    disable: () => run(() => api.companionDisable()),
    revoke: () => run(() => api.companionRevoke()),
    rename,
  };
}
