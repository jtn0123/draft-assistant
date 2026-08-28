import type { ChatOptions, ChatUsage } from "../types";
import { describeOptions, EFFORTS, formatSeconds, formatTokens, MODELS } from "./chatOptions";

/** Model, effort, speed, and web-search choices, folded up by default. */
export function ChatSettings({
  options,
  onChange,
  disabled,
}: {
  options: ChatOptions;
  onChange: (next: ChatOptions) => void;
  disabled: boolean;
}) {
  const set = (patch: Partial<ChatOptions>) => onChange({ ...options, ...patch });
  const modelHint = MODELS.find((m) => m.id === options.model)?.hint;
  return (
    <details className="chat-settings">
      <summary>
        <span className="chat-settings-label">Settings</span>
        <span className="muted">{describeOptions(options)}</span>
      </summary>
      <div className="chat-settings-body">
        <label>
          Model
          <select
            aria-label="Model"
            value={options.model}
            disabled={disabled}
            onChange={(e) => set({ model: e.target.value })}
          >
            {MODELS.map((m) => (
              <option key={m.id} value={m.id}>
                {m.label}
              </option>
            ))}
          </select>
          {modelHint && <span className="muted small-text">{modelHint}</span>}
        </label>
        <label>
          Thinking effort
          <select
            aria-label="Thinking effort"
            value={options.effort ?? ""}
            disabled={disabled}
            onChange={(e) => set({ effort: e.target.value || null })}
          >
            {EFFORTS.map((ef) => (
              <option key={ef.id} value={ef.id}>
                {ef.label}
              </option>
            ))}
          </select>
          <span className="muted small-text">
            Higher thinks longer before answering. Low is enough for lookups.
          </span>
        </label>
        <label className="chat-check">
          <input
            type="checkbox"
            aria-label="Fast mode"
            checked={options.fast}
            disabled={disabled}
            onChange={(e) => set({ fast: e.target.checked })}
          />
          Fast mode
          <span className="muted small-text">
            Same model, faster output. Needs extra usage enabled on the account.
          </span>
        </label>
        <label className="chat-check">
          <input
            type="checkbox"
            aria-label="Web search"
            checked={options.web_search}
            disabled={disabled}
            onChange={(e) => set({ web_search: e.target.checked })}
          />
          Web search
          <span className="muted small-text">
            Off by default. Turn on to ask why someone is flagged, or for news the
            board cannot hold. Slower.
          </span>
        </label>
      </div>
    </details>
  );
}

/** What the last answer cost and how big the thread has grown. */
export function UsageLine({
  usage,
  questions,
  cost,
}: {
  usage: ChatUsage | null;
  questions: number;
  cost: number;
}) {
  if (usage === null) return null;
  const model = MODELS.find((m) => m.id === usage.model)?.label ?? usage.model;
  const parts = [
    `Context ${formatTokens(usage.context_tokens)} tokens`,
    formatSeconds(usage.duration_ms),
    model,
    `${questions} question${questions === 1 ? "" : "s"}`,
  ];
  if (cost > 0) parts.push(`$${cost.toFixed(2)}`);
  if (usage.web_searches > 0) {
    parts.push(`${usage.web_searches} web search${usage.web_searches === 1 ? "" : "es"}`);
  }
  return (
    <p className="chat-usage muted" aria-label="Usage">
      {parts.join(" · ")}
    </p>
  );
}
