// The `Api` implementation a follower runs against somebody else's desktop
// app: the same calls the UI already makes, answered over HTTP and one shared
// WebSocket instead of Tauri's IPC.
//
// Two rules shape all of it. Reads mirror the endpoints in COMPANION-API.md
// one for one, so the screens cannot tell the difference. Writes — keys,
// budget, Yahoo, league switching, drafting, the local Ask Claude — are the
// host's alone and are refused here by name, because a follower silently
// half-doing them would be worse than a follower that says who is in charge.

import type { UnlistenFn } from "@tauri-apps/api/event";
import type { Api } from "./api";
import { validateDraftView, validateSeasonView } from "./api";
import { clearFollow } from "./companion";
import { REVOKED_CLOSE_CODE, setFollowStatus } from "./followStatus";
import type { ChatSettings } from "./chat-types";
import type { SeasonView } from "./season-types";
import type {
  AppConfig,
  CompanionDevice,
  DraftView,
  FollowRecord,
  PollHealth,
  RemoteConfig,
  SharedChatThread,
} from "./types";

/** Set when a host drops this device, and read once by the shell on the
 *  reload that follows, so the user is told rather than just demoted. */
export const REVOKED_KEY = "da.companion.revoked";

/** How long to wait before each reconnection attempt, in ms; the last one
 *  repeats for as long as the host stays away. */
const BACKOFF_MS = [1000, 2000, 5000, 10000];
const PING_MS = 25000;

/** Frames the host sends down the socket. */
type Frame = { type: string; payload?: unknown };

/** Bytes to a `data:` URL, the shape every image in the UI already takes. */
function dataUri(type: string, bytes: ArrayBuffer): string {
  const view = new Uint8Array(bytes);
  let binary = "";
  for (const byte of view) binary += String.fromCharCode(byte);
  return `data:${type || "image/jpeg"};base64,${btoa(binary)}`;
}

/** A GET that knows about the host's two failure modes: a 404 for something
 *  not loaded yet, and a 401 for a device that is no longer paired. */
export function remoteFetcher(follow: FollowRecord, onRevoked: () => void) {
  return async function fetchJson<T>(path: string): Promise<T | null> {
    const response = await fetch(`${follow.url}${path}`, {
      headers: { authorization: `Bearer ${follow.token}` },
    });
    if (response.status === 401) {
      onRevoked();
      throw new Error("The host revoked this device");
    }
    if (response.status === 404) return null;
    if (!response.ok) throw new Error(`${follow.host_name} answered ${response.status}`);
    return (await response.json()) as T;
  };
}

/** The one socket every screen's live updates come down, reconnecting on its
 *  own for as long as this window is open. */
class HostSocket {
  private socket: WebSocket | null = null;
  private handlers = new Map<string, Set<(payload: unknown) => void>>();
  private attempt = 0;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private ping: ReturnType<typeof setInterval> | undefined;
  private closed = false;
  /** Run on every open, including each reconnection. Set by `remoteApi`
   *  after construction so the re-read can name the socket it belongs to. */
  onOpen: () => void = () => undefined;

  constructor(
    private follow: FollowRecord,
    private onRevoked: () => void,
  ) {}

  on(type: string, handler: (payload: unknown) => void): UnlistenFn {
    const set = this.handlers.get(type) ?? new Set();
    set.add(handler);
    this.handlers.set(type, set);
    this.open();
    return () => set.delete(handler);
  }

  open(): void {
    if (this.socket !== null || this.closed) return;
    const url = `${this.follow.url.replace(/^http/, "ws")}/api/events?token=${encodeURIComponent(this.follow.token)}`;
    const socket = new WebSocket(url);
    this.socket = socket;
    socket.onopen = () => {
      this.attempt = 0;
      setFollowStatus("connected");
      this.ping = setInterval(() => socket.send(JSON.stringify({ type: "ping" })), PING_MS);
      this.onOpen();
    };
    socket.onmessage = (event: MessageEvent) => this.deliver(String(event.data));
    socket.onclose = (event?: { code?: number }) => {
      clearInterval(this.ping);
      this.socket = null;
      // A host that has forgotten this device closes with 4401 rather than
      // dropping the connection. Reconnecting into that forever would be a
      // device arguing with a decision that has already been made.
      if (event?.code === REVOKED_CLOSE_CODE) {
        this.stop();
        this.onRevoked();
        return;
      }
      setFollowStatus("reconnecting");
      this.retry();
    };
    // A socket that errors closes straight after; the close handler is the
    // one place that schedules a retry, so there is only ever one pending.
    socket.onerror = () => socket.close();
  }

  /** True when something on screen is listening for this kind of frame. */
  wants(type: string): boolean {
    return (this.handlers.get(type)?.size ?? 0) > 0;
  }

  /** Hand a payload to the handlers a frame of that type would have reached,
   *  so a snapshot fetched over HTTP arrives the same way a pushed one does. */
  emit(type: string, payload: unknown): void {
    for (const handler of this.handlers.get(type) ?? []) handler(payload);
  }

  private deliver(text: string): void {
    let frame: Frame;
    try {
      frame = JSON.parse(text) as Frame;
    } catch {
      return;
    }
    if (frame.type === "revoked") {
      this.stop();
      this.onRevoked();
      return;
    }
    for (const handler of this.handlers.get(frame.type) ?? []) handler(frame.payload);
  }

  private retry(): void {
    if (this.closed) return;
    const wait = BACKOFF_MS[Math.min(this.attempt, BACKOFF_MS.length - 1)] ?? 10000;
    this.attempt += 1;
    this.timer = setTimeout(() => this.open(), wait);
  }

  stop(): void {
    this.closed = true;
    clearTimeout(this.timer);
    clearInterval(this.ping);
    this.socket?.close();
    this.socket = null;
  }
}

/** The name on the error a follower raises when the host has no season open. */
export const NO_SEASON_ON_HOST = "NoSeasonOnHost";

/** Not a failure so much as a state: the host has not opened its Season
 *  screen, and nothing on a follower can make it. The shell shows this in
 *  place rather than as a toast. */
function noSeasonOnHost(hostName: string): Error {
  const error = new Error(
    `${hostName} hasn't opened the Season screen yet — the season shows here once it does`,
  );
  error.name = NO_SEASON_ON_HOST;
  return error;
}

/** What every host-only call says, naming the machine that owns the setting. */
function hostOnly(hostName: string): Promise<never> {
  return Promise.reject(new Error(`That's controlled by the host (${hostName})`));
}

/** What a dropped follower does when the shell has not said otherwise:
 *  forget the host, and leave the note the next launch reads. The window is
 *  left standing rather than reloaded out from under whoever is looking at
 *  it — the header says what happened and offers the way back. */
function forgetHost(): void {
  clearFollow();
  try {
    localStorage.setItem(REVOKED_KEY, "1");
  } catch {
    // The note is a nicety; the demotion above is the point.
  }
}

/**
 * Build the follower's backend.
 *
 * `onRevoked` defaults to `forgetHost`. Either way the connection state moves
 * to "revoked" first, because that is what the header reads and no caller
 * should have to remember to set it.
 */
export function remoteApi(follow: FollowRecord, onRevoked?: () => void): Api {
  const revoked = () => {
    setFollowStatus("revoked");
    (onRevoked ?? forgetHost)();
  };
  const fetchJson = remoteFetcher(follow, revoked);
  const socket = new HostSocket(follow, revoked);
  const refused = () => hostOnly(follow.host_name);

  const state = async (): Promise<DraftView> => {
    const view = await fetchJson<DraftView>("/api/state");
    if (view === null) throw new Error(`${follow.host_name} has no league loaded`);
    return validateDraftView(view);
  };
  const season = async (): Promise<SeasonView> => {
    const view = await fetchJson<SeasonView>("/api/season");
    if (view === null) throw noSeasonOnHost(follow.host_name);
    return validateSeasonView(view);
  };
  const image = async (path: string): Promise<string | null> => {
    try {
      const response = await fetch(`${follow.url}${path}`, {
        headers: { authorization: `Bearer ${follow.token}` },
      });
      if (!response.ok) return null;
      return dataUri(response.headers.get("content-type") ?? "", await response.arrayBuffer());
    } catch {
      return null;
    }
  };
  const listen = <T>(type: string, handler: (value: T) => void): Promise<UnlistenFn> =>
    Promise.resolve(socket.on(type, (payload) => handler(payload as T)));

  // Every reconnection re-reads what the screens are showing. A socket that
  // was away for ten seconds missed whatever happened in them, and a board
  // that quietly sits on last minute's picks is the worst way to find out.
  // The host sends both snapshots as opening frames too; this window cannot
  // tell how old the host it joined is, so it asks either way. A failure is
  // dropped on purpose: nothing here is worth a toast, and the socket is
  // already live for whatever comes next.
  socket.onOpen = () => {
    if (socket.wants("draft-updated")) {
      void state().then(
        (view) => socket.emit("draft-updated", view),
        () => undefined,
      );
    }
    if (socket.wants("season-updated")) {
      void season().then(
        (view) => socket.emit("season-updated", view),
        () => undefined,
      );
    }
  };

  return {
    // ---------- reads ----------
    getState: state,
    refreshPicks: state,
    refreshData: state,
    getSeason: season,
    loadSeason: season,
    refreshSeason: season,
    getConfig: async () => {
      const config = await fetchJson<RemoteConfig>("/api/config");
      return remoteConfig(config);
    },
    headshot: (playerId) => image(`/api/headshot/${encodeURIComponent(playerId)}`),
    avatar: (reference, full) =>
      image(`/api/avatar/${encodeURIComponent(reference)}${full ? "?full=1" : ""}`),

    // ---------- live ----------
    onDraftUpdated: (handler) =>
      listen<DraftView>("draft-updated", (view) => handler(validateDraftView(view))),
    onSeasonUpdated: (handler) =>
      listen<SeasonView>("season-updated", (view) => handler(validateSeasonView(view))),
    onPollHealth: (handler) => listen<PollHealth>("poll-health", handler),
    onSeasonPollHealth: (handler) => listen<PollHealth>("season-poll-health", handler),
    // The host polls; a follower is only ever told about it.
    startPolling: () => Promise.resolve(),
    stopPolling: () => Promise.resolve(),
    startSeasonPolling: () => Promise.resolve(),
    stopSeasonPolling: () => Promise.resolve(),

    // ---------- shared chat ----------
    sharedChatGet: async (screen) => {
      const thread = await fetchJson<SharedChatThread>(
        `/api/chat?screen=${encodeURIComponent(screen)}`,
      );
      return thread ?? { league_id: "", screen, busy: false, entries: [] };
    },
    sharedChatSend: async (screen, text) => {
      const response = await fetch(`${follow.url}/api/chat`, {
        method: "POST",
        headers: { authorization: `Bearer ${follow.token}`, "content-type": "application/json" },
        body: JSON.stringify({ screen, text }),
      });
      if (response.status === 401) {
        revoked();
        throw new Error("The host revoked this device");
      }
      if (response.status === 409)
        throw new Error("Someone else is asking — try again in a moment");
      if (response.status === 429) throw new Error("That's a lot of questions. Give it a minute.");
      if (!response.ok) throw new Error(`${follow.host_name} answered ${response.status}`);
    },
    onSharedChat: (handler) => listen<SharedChatThread>("shared-chat", handler),
    onCompanionDevices: (handler) => listen<CompanionDevice[]>("devices", handler),

    // ---------- the host's alone ----------
    // The shell "adds" its active league on every boot to restore it. On a
    // follower that league is whatever the host has open, so the call is a
    // read: hand back the host's board, and refuse only a *different* id,
    // which really would be asking the host to switch.
    addLeague: async (leagueId: string) => {
      const view = await state();
      if (view.league.league_id !== leagueId) {
        throw new Error(
          `${follow.host_name} is on a different league; switching is up to the host`,
        );
      }
      return view;
    },
    removeLeague: refused,
    setMyUsername: refused,
    setApiKey: refused,
    setChatBudget: refused,
    setChatProvider: refused,
    yahooSaveCredentials: refused,
    yahooBeginConnect: refused,
    yahooFinishConnect: refused,
    yahooDisconnect: refused,
    yahooLeagues: refused,
    importSecondOpinion: refused,
    recordManualPick: refused,
    undoManualPick: refused,
    clearKeepers: refused,
    exportState: refused,
    askClaude: refused,
    companionStatus: refused,
    companionEnable: refused,
    companionDisable: refused,
    companionRevoke: refused,
    setDeviceName: refused,

    // ---------- nothing to ask, nothing to fail ----------
    sleeperLeagues: () => Promise.resolve([]),
    yahooStatus: () =>
      Promise.resolve({ configured: false, connected: false, redirect: "oob", account: null }),
    chatSuggestions: () => Promise.resolve([]),
    // The follower never runs the local composer, so this only has to be
    // something the panel can render without asking for a key.
    chatSettings: () => Promise.resolve(followerChatSettings()),
  };
}

/** `/api/config` is deliberately thinner than the desktop's own config — no
 *  keys, no budget — so the missing halves are defaulted here rather than
 *  left undefined for a screen to trip over. */
function remoteConfig(config: RemoteConfig | null): AppConfig {
  return {
    my_user_id: config?.my_user_id ?? null,
    active_league_id: config?.active_league_id ?? null,
    leagues: config?.leagues ?? [],
  };
}

function followerChatSettings(): ChatSettings {
  return {
    // "The host has one" — the follower never sends a key of its own, and a
    // key form on this screen would be asking for something it cannot use.
    has_key: true,
    key_hint: null,
    cli_available: false,
    provider: "api",
    key_store: "file",
    budget_usd: 0,
    spend_usd: {},
    models: ["Opus 5"],
    efforts: { "Opus 5": ["High"] },
    notes: {},
  };
}
