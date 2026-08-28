import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppConfig,
  ChatOptions,
  ChatReply,
  ChatTurn,
  DraftView,
  PollHealth,
} from "./types";

const DRAFT_VIEW_SCHEMA_VERSION = "1.3";

export function validateDraftView(value: DraftView): DraftView {
  if (value.schema_version !== DRAFT_VIEW_SCHEMA_VERSION) {
    throw new Error(
      `Incompatible draft data: expected schema ${DRAFT_VIEW_SCHEMA_VERSION}, received ${value.schema_version || "missing"}. Update and restart the app.`,
    );
  }
  return value;
}

async function invokeView(command: string, args?: Record<string, unknown>): Promise<DraftView> {
  return validateDraftView(await invoke<DraftView>(command, args));
}

/** True when running inside the Tauri shell (vs a plain browser tab). */
const inTauri = "__TAURI_INTERNALS__" in window;

interface Api {
  /** True outside the Tauri shell: fixture data, nothing can be changed. */
  preview: boolean;
  /**
   * Browser preview only: a URL polled for fresh state dumps, set by
   * `?replay=<url>`. Lets the real UI follow a replayed draft
   * (`scripts/replay-sleeper.mjs`) without the desktop shell.
   */
  replay: string | null;
  /**
   * Browser preview only: a recorded Ask Claude session (`?chat=<url>`, as
   * written by `dump_state --chat-out`) played back one exchange per
   * question, so the panel can be seen and tested without the CLI.
   */
  chatRecording: string | null;
  addLeague(leagueId: string, force?: boolean): Promise<DraftView>;
  setMyUsername(username: string): Promise<string>;
  getConfig(): Promise<AppConfig>;
  getState(): Promise<DraftView>;
  refreshPicks(): Promise<DraftView>;
  refreshData(): Promise<DraftView>;
  recordManualPick(playerId: string): Promise<DraftView>;
  undoManualPick(): Promise<DraftView>;
  exportState(): Promise<string>;
  /**
   * Ask Claude. `onText` receives each piece of the answer as it is written;
   * the resolved reply carries the whole answer.
   */
  chat(
    question: string,
    history: ChatTurn[],
    options: ChatOptions,
    onText?: (text: string) => void,
  ): Promise<ChatReply>;
  chatCompact(history: ChatTurn[], options: ChatOptions): Promise<ChatReply>;
  startPolling(intervalSecs?: number): Promise<void>;
  stopPolling(): Promise<void>;
  /**
   * Live views from the poller. A payload that fails the schema check is
   * reported through `onError` — thrown inside an event callback it would be
   * swallowed, and live updates would silently stop.
   */
  onDraftUpdated(
    handler: (view: DraftView) => void,
    onError?: (error: unknown) => void,
  ): Promise<UnlistenFn>;
  onPollHealth(handler: (health: PollHealth) => void): Promise<UnlistenFn>;
}

const tauriApi: Api = {
  preview: false,
  replay: null,
  chatRecording: null,
  addLeague: (leagueId, force = false) =>
    invokeView("add_league", { leagueId, force }),
  setMyUsername: (username) => invoke<string>("set_my_username", { username }),
  getConfig: () => invoke<AppConfig>("get_config"),
  getState: () => invokeView("get_state"),
  refreshPicks: () => invokeView("refresh_picks"),
  refreshData: () => invokeView("refresh_data"),
  recordManualPick: (playerId) =>
    invokeView("record_manual_pick", { playerId }),
  undoManualPick: () => invokeView("undo_manual_pick"),
  exportState: () => invoke<string>("export_state"),
  chat: (question, history, options, onText) => {
    const channel = new Channel<string>();
    channel.onmessage = (text) => onText?.(text);
    return invoke<ChatReply>("chat", { question, history, options, onText: channel });
  },
  chatCompact: (history, options) =>
    invoke<ChatReply>("chat_compact", { history, options }),
  startPolling: (intervalSecs = 3) =>
    invoke<void>("start_polling", { intervalSecs }),
  stopPolling: () => invoke<void>("stop_polling"),
  onDraftUpdated: (handler, onError) =>
    listen<DraftView>("draft-updated", (event) => {
      try {
        handler(validateDraftView(event.payload));
      } catch (error) {
        onError?.(error);
      }
    }),
  onPollHealth: (handler) =>
    listen<PollHealth>("poll-health", (event) => handler(event.payload)),
};

/**
 * Browser fallback for UI development: serves a captured real state dump
 * (public/dev-fixture.json) so the full interface renders outside Tauri.
 * Mutating calls only simulate what they can locally.
 */
const REPLAY_POLL_MS = 3000;

/** How a recorded answer is paced when played back: word by word, briskly. */
const RECORDING_CHUNK_MS = 12;

interface RecordedExchange {
  question: string;
  answer: string;
  usage: ChatReply["usage"];
  as_of: ChatReply["as_of"];
}

function browserApi(): Api {
  const params = new URLSearchParams(window.location.search);
  const replay = params.get("replay");
  const chatRecording = params.get("chat");
  let recording: RecordedExchange[] | null = null;
  let played = 0;
  // Play back the next recorded exchange, streaming its answer so the panel
  // behaves as it does against the real CLI.
  const playRecording = async (onText?: (text: string) => void): Promise<ChatReply> => {
    if (chatRecording === null) {
      throw new Error("browser preview cannot reach the Claude CLI — run the desktop app");
    }
    if (recording === null) {
      const resp = await fetch(chatRecording, { cache: "no-store" });
      if (!resp.ok) throw new Error(`chat recording ${chatRecording} returned ${resp.status}`);
      recording = (await resp.json()) as RecordedExchange[];
    }
    const next = recording[played];
    if (!next) throw new Error("the recorded session has no more answers — start a new chat");
    played += 1;
    const words = next.answer.split(/(?<=\s)/);
    for (const word of words) {
      await new Promise((resolve) => window.setTimeout(resolve, RECORDING_CHUNK_MS));
      onText?.(word);
    }
    return { answer: next.answer, usage: next.usage, as_of: next.as_of };
  };
  let cached: DraftView | null = null;
  const source = replay ?? "/dev-fixture.json";
  const load = async (): Promise<DraftView> => {
    const resp = await fetch(source, { cache: "no-store" });
    if (!resp.ok) {
      throw new Error(
        replay
          ? `replay source ${source} returned ${resp.status}`
          : "dev fixture missing (browser preview only works with public/dev-fixture.json)",
      );
    }
    // A missing path under the dev server answers 200 with index.html, so the
    // parse failure is the common case here. Its raw message ("Unexpected
    // token '<'") means nothing to whoever is looking at the screen.
    let body: unknown;
    try {
      body = await resp.json();
    } catch {
      throw new Error(
        `could not read draft state from ${source} — it is not a state dump (check the path, and that the replay server is writing it)`,
      );
    }
    return validateDraftView(body as DraftView);
  };
  // Replay mode: poll the source and push newer dumps through the same
  // listener the desktop poller uses. Each `dump_state` run restarts `seq` at
  // 1, so `generated_at` orders the dumps instead.
  const viewHandlers: ((view: DraftView) => void)[] = [];
  const healthHandlers: ((health: PollHealth) => void)[] = [];
  let timer: number | undefined;
  let lastGeneratedAt = 0;
  const fixture = async (): Promise<DraftView> => {
    if (cached === null) {
      cached = await load();
      lastGeneratedAt = cached.generated_at;
    }
    return cached;
  };
  const poll = async () => {
    try {
      const next = await load();
      if (next.generated_at <= lastGeneratedAt) return;
      lastGeneratedAt = next.generated_at;
      next.seq = next.generated_at;
      for (const handler of viewHandlers) handler(next);
      for (const handler of healthHandlers) {
        handler({ last_success_at: next.generated_at, consecutive_failures: 0, last_error: null });
      }
    } catch {
      // A half-written dump parses badly for a moment; the next poll reads it.
    }
  };

  return {
    preview: true,
    replay,
    chatRecording,
    addLeague: fixture,
    setMyUsername: async (u) => u,
    getConfig: async () => {
      const v = await fixture();
      return {
        my_user_id: "browser-preview",
        active_league_id: v.league.league_id,
        leagues: [
          {
            league_id: v.league.league_id,
            name: v.league.name,
            season: v.league.season,
          },
        ],
      };
    },
    getState: fixture,
    refreshPicks: fixture,
    // Only the replay dump can actually be re-read here; projections are
    // refetched by the engine, which the preview does not have.
    refreshData: async () => {
      if (!replay) {
        throw new Error(
          "browser preview is read-only — run the desktop app to refresh projections",
        );
      }
      cached = await load();
      lastGeneratedAt = cached.generated_at;
      return cached;
    },
    recordManualPick: async () => {
      throw new Error("browser preview is read-only — run the desktop app to draft");
    },
    undoManualPick: async () => {
      throw new Error("browser preview is read-only — run the desktop app to draft");
    },
    exportState: async () => {
      throw new Error("browser preview is read-only — run the desktop app to export state");
    },
    chat: (_question, _history, _options, onText) => playRecording(onText),
    chatCompact: async () => {
      throw new Error("browser preview cannot reach the Claude CLI — run the desktop app");
    },
    startPolling: async () => {
      if (!replay) {
        throw new Error("browser preview is read-only — live sync requires the desktop app");
      }
      window.clearInterval(timer);
      timer = window.setInterval(() => void poll(), REPLAY_POLL_MS);
    },
    stopPolling: async () => {
      window.clearInterval(timer);
      timer = undefined;
    },
    onDraftUpdated: async (handler) => {
      viewHandlers.push(handler);
      return () => {
        viewHandlers.splice(viewHandlers.indexOf(handler), 1);
      };
    },
    onPollHealth: async (handler) => {
      healthHandlers.push(handler);
      return () => {
        healthHandlers.splice(healthHandlers.indexOf(handler), 1);
      };
    },
  };
}

export const api: Api = inTauri ? tauriApi : browserApi();
