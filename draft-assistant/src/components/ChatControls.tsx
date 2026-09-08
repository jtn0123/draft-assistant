// The row of pickers above the thread: which model answers and how hard it
// thinks. Split out of `Chat.tsx` so the panel file stays the panel.
//
// There is no route picker any more. The API route billed a key per token for
// answers the Claude Code subscription already covers, so the backend takes
// the CLI whenever it is installed and the choice is not offered.

import { useId, useRef, useState } from "react";
import type { ChatSettings } from "../chat-types";

/** How an effort level is written on its button. The wire values are the
 *  API's own (`xhigh`), and the backend sends them through verbatim — so
 *  `xhigh` was the one button in the row set in lowercase, reading like a bug
 *  rather than a level. Only the label changes; the value sent is untouched. */
const EFFORT_LABEL: Record<string, string> = { xhigh: "X-High" };

/** Model-button tooltips, from the design. */
const MODEL_TITLE: Record<string, string> = {
  "GPT-6 Astra": "GPT-6 Astra, smarter, through your ChatGPT-authenticated Codex CLI",
  "GPT-5.6 Sol": "GPT-5.6 Sol through your ChatGPT-authenticated Codex CLI",
  "Opus 5": "Claude Opus 5: adaptive thinking, supports all five effort levels",
  "Fable 5.1":
    "Claude Fable 5.1: slower and smarter. Thinking can't be turned off, effort is the only depth control",
};

/** The short word beside a model's name: what picking it trades. */
const MODEL_HINT: Record<string, string> = {
  "Fable 5.1": "slower, smarter",
  "GPT-6 Astra": "smarter",
};

export function ChatControls({
  settings,
  models,
  model,
  onModel,
  efforts,
  effort,
  onEffort,
}: {
  settings: ChatSettings | null;
  models: string[];
  model: string;
  onModel: (name: string) => void;
  efforts: string[];
  effort: string;
  onEffort: (level: string) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const modelOptions = useId();
  const toggle = useRef<HTMLButtonElement>(null);
  const chooseModel = (name: string) => {
    onModel(name);
    setExpanded(false);
    toggle.current?.focus();
  };
  return (
    <div className="chat-controls">
      <button
        type="button"
        className="btn-ghost btn-row chat-model-toggle"
        ref={toggle}
        aria-label={`Model: ${model}`}
        aria-expanded={expanded}
        aria-controls={modelOptions}
        onClick={() => setExpanded((open) => !open)}
      >
        {model} <span aria-hidden="true">{expanded ? "▴" : "▾"}</span>
      </button>
      {expanded && (
        <div
          className="segmented chat-model-options"
          id={modelOptions}
          role="group"
          aria-label="Model"
        >
          {models.map((name) => (
            <button
              key={name}
              type="button"
              className={name === model ? "seg is-on" : "seg"}
              onClick={() => chooseModel(name)}
              onKeyDown={(event) => {
                if (event.key === "Escape") {
                  setExpanded(false);
                  toggle.current?.focus();
                }
              }}
              title={MODEL_TITLE[name]}
              aria-label={name}
              aria-pressed={name === model}
            >
              {name}
              {MODEL_HINT[name] && (
                <span className="muted chat-model-hint"> · {MODEL_HINT[name]}</span>
              )}
            </button>
          ))}
        </div>
      )}
      <span className="muted chat-model-note">
        {model.startsWith("GPT-")
          ? "via ChatGPT (Codex)"
          : model === "Fable 5.1"
            ? "thinking always on"
            : "adaptive thinking"}
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
    </div>
  );
}
