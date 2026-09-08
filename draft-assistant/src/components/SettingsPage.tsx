import { useEffect, useRef, type ReactNode } from "react";
import type { SettingsRow } from "./HeaderSettingsRow";
import { useFocusTrap } from "./useFocusTrap";
import "../settings-page.css";

const SECTIONS = [
  {
    id: "draft",
    title: "Draft identity",
    note: "Your league and the account used for your roster and turn.",
    rows: ["league", "username"],
  },
  {
    id: "connections",
    title: "Remote connections",
    note: "Share this Mac with a phone, or follow another Draft Assistant.",
    rows: ["companion", "join-host", "leave-host", "yahoo"],
  },
  {
    id: "appearance",
    title: "Appearance & sound",
    note: "Make the board comfortable to use during the draft.",
    rows: ["appearance", "avatars", "chime", "ask"],
  },
  {
    id: "data",
    title: "Draft data",
    note: "Sync, projections and local draft corrections.",
    rows: ["polling", "refresh", "import-csv", "clear-keepers", "export"],
  },
  {
    id: "diagnostics",
    title: "Diagnostics & updates",
    note: "Connection details, logs and the installed app version.",
    rows: ["diagnostics", "updates", "version"],
  },
];

function PageRow({ row }: { row: SettingsRow }) {
  const text = (
    <span className="settings-page-row-text">
      <span>{row.label}</span>
      <span className="muted">{row.note}</span>
    </span>
  );
  if (row.kind === "radio")
    return (
      <div className="settings-page-row">
        {text}
        <div className="settings-radio" role="group" aria-label={row.label}>
          {row.options?.map((option) => (
            <button
              key={option.id}
              type="button"
              className={option.on ? "settings-radio-option is-on" : "settings-radio-option"}
              aria-pressed={option.on}
              onClick={option.onSelect}
            >
              {option.label}
            </button>
          ))}
        </div>
      </div>
    );
  const toggle = row.kind === "toggle";
  return (
    <button
      type="button"
      className="settings-page-row"
      role={toggle ? "switch" : undefined}
      aria-checked={toggle ? row.on : undefined}
      onClick={row.onSelect}
    >
      {text}
      <span className={row.on ? "settings-row-value is-on" : "settings-row-value"}>
        {row.value}
      </span>
    </button>
  );
}

export function SettingsPage({
  rows,
  identity,
  leagueName,
  screen,
  onClose,
}: {
  rows: SettingsRow[];
  identity?: ReactNode;
  leagueName: string;
  screen: "draft" | "season";
  onClose: () => void;
}) {
  const sections = SECTIONS.map((section) => ({
    ...section,
    items: rows.filter((row) => section.rows.includes(row.id)),
  })).filter((section) => section.items.length > 0);
  const page = useRef<HTMLDivElement>(null);
  const back = useRef<HTMLButtonElement>(null);
  useFocusTrap(page, onClose);
  useEffect(() => {
    back.current?.focus();
    return () => document.querySelector<HTMLButtonElement>('button[title="Settings"]')?.focus();
  }, []);
  return (
    <div className="settings-page-wrap">
      <div
        className="settings-page"
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-page-title"
        ref={page}
      >
        <header className="settings-page-head">
          <div>
            <span className="eyebrow">{leagueName}</span>
            <h1 id="settings-page-title">Settings</h1>
            <p className="muted">
              Live sync continues when enabled. Return to your draft at any time.
            </p>
          </div>
          <button type="button" className="btn-ghost" ref={back} onClick={onClose}>
            Back to {screen}
          </button>
        </header>
        <nav className="settings-page-nav" aria-label="Settings sections">
          {sections.map((section) => (
            <a key={section.id} href={`#settings-${section.id}`}>
              {section.title}
            </a>
          ))}
        </nav>
        <div className="settings-page-body">
          {sections.map((section) => {
            return (
              <section
                key={section.id}
                id={`settings-${section.id}`}
                className="settings-page-section"
                aria-labelledby={`settings-${section.id}-title`}
              >
                <div className="settings-page-section-heading">
                  <h2 id={`settings-${section.id}-title`}>{section.title}</h2>
                  <p className="muted">{section.note}</p>
                </div>
                <div className="settings-page-controls">
                  {section.items.map((row) => (
                    <div key={row.id}>
                      <PageRow row={row} />
                      {row.id === "username" && identity}
                    </div>
                  ))}
                </div>
              </section>
            );
          })}
        </div>
      </div>
    </div>
  );
}
