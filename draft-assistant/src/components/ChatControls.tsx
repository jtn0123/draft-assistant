// The row of pickers above the thread: which model answers, how hard it
// thinks, and — when the CLI is installed — which route the question takes.
// Split out of `Chat.tsx` so the panel file stays the panel.

import type { ChatSettings } from "../chat-types";

/** The two ways an answer can reach Claude. */
const PROVIDERS: [id: "claude_code" | "api", name: string, title: string][] = [
  [
    "claude_code",
    "Claude Code",
    "Runs the Claude Code CLI installed on this Mac, signed in with your Claude subscription — no API key needed",
  ],
  ["api", "API key", "Calls the Anthropic API directly with the key stored in this app"],
];

/** How an effort level is written on its button. The wire values are the
 *  API's own (`xhigh`), and the backend sends them through verbatim — so
 *  `xhigh` was the one button in the row set in lowercase, reading like a bug
 *  rather than a level. Only the label changes; the value sent is untouched. */
const EFFORT_LABEL: Record<string, string> = { xhigh: "X-High" };

/** Model-button tooltips, from the design. */
const MODEL_TITLE: Record<string, string> = {
  "Opus 5": "Claude Opus 5 — adaptive thinking, supports all five effort levels",
  "Fable 5":
    "Claude Fable 5 — Mythos-class; thinking can't be turned off, effort is the only depth control",
};

export function ChatControls({
  settings,
  models,
  model,
  onModel,
  efforts,
  effort,
  onEffort,
  onProvider,
}: {
  settings: ChatSettings | null;
  models: string[];
  model: string;
  onModel: (name: string) => void;
  efforts: string[];
  effort: string;
  onEffort: (level: string) => void;
  onProvider: (id: "api" | "claude_code") => void;
}) {
  return (
    <div className="chat-controls">
      <div className="segmented" role="group" aria-label="Model">
        {models.map((name) => (
          <button
            key={name}
            type="button"
            className={name === model ? "seg is-on" : "seg"}
            onClick={() => onModel(name)}
            title={MODEL_TITLE[name]}
            aria-pressed={name === model}
          >
            {name}
          </button>
        ))}
      </div>
      <span className="muted chat-model-note">
        {model === "Fable 5" ? "thinking always on" : "adaptive thinking"}
      </span>
      <span className="label chat-effort-label">Effort</span>
      <div className="segmented" role="group" aria-label="Effort">
        {efforts.map((level) => (
          <button
            key={level}
            type="button"
            className={level === effort ? "seg is-on" : "seg"}
            onClick={() => onEffort(level)}
            title={settings?.notes[level]?.[0]}
            aria-pressed={level === effort}
          >
            {EFFORT_LABEL[level] ?? level}
          </button>
        ))}
      </div>
      {settings?.cli_available && (
        <>
          <span className="label chat-effort-label">Via</span>
          <div className="segmented" role="group" aria-label="Route">
            {PROVIDERS.map(([id, name, title]) => (
              <button
                key={id}
                type="button"
                className={id === settings.provider ? "seg is-on" : "seg"}
                onClick={() => onProvider(id)}
                title={title}
                aria-pressed={id === settings.provider}
              >
                {name}
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
