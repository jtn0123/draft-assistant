// League header: identity, Draft/Season toggle, sync status, and the
// settings menu that hangs off it.

import { useEffect, useRef } from "react";
import type { PollHealth } from "../types";
import { age } from "../format";

export type Screen = "draft" | "season";

export interface SettingsRow {
  label: string;
  note: string;
  value: string;
  on: boolean;
  onSelect: () => void;
}

function SyncPill({ polling, health }: { polling: boolean; health: PollHealth | null }) {
  const failures = health?.consecutive_failures ?? 0;
  if (!polling) {
    return (
      <span className="pill pill-off">
        <span className="dot" />
        Live sync off
      </span>
    );
  }
  if (failures >= 2) {
    return (
      <span className="pill pill-stale" title={health?.last_error ?? undefined}>
        <span className="dot" />
        Sync stale · {failures} failures
      </span>
    );
  }
  if (failures === 1) {
    return (
      <span className="pill pill-stale" title={health?.last_error ?? undefined}>
        <span className="dot" />
        Sync retrying
      </span>
    );
  }
  return (
    <span className="pill pill-live">
      <span className="dot" />
      Live · {age(health?.last_success_at ?? null)}
    </span>
  );
}

function UndoIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 14 14"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M2.5 5.5h6.2a3 3 0 0 1 0 6H5" />
      <path d="M4.8 3.2 2.4 5.5l2.4 2.3" />
    </svg>
  );
}

function ChimeIcon({ on }: { on: boolean }) {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 14 14"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M2 5.4h2.1L7 3v8L4.1 8.6H2z" />
      {on ? (
        <path d="M9.4 5.1a2.6 2.6 0 0 1 0 3.8M11 3.6a4.8 4.8 0 0 1 0 6.8" />
      ) : (
        <path d="M9.6 5.4l3 3.2M12.6 5.4l-3 3.2" />
      )}
    </svg>
  );
}

function GearIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 14 14"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      aria-hidden="true"
    >
      <circle cx="7" cy="7" r="2.1" />
      <path d="M7 1.4v1.4M7 11.2v1.4M1.4 7h1.4M11.2 7h1.4M3 3l1 1M10 10l1 1M11 3l-1 1M4 10l-1 1" />
    </svg>
  );
}

export function Header({
  leagueName,
  subtitle,
  meta,
  screen,
  onScreen,
  polling,
  pollHealth,
  chime,
  onToggleChime,
  onUndo,
  chatOpen,
  onToggleChat,
  settingsOpen,
  onToggleSettings,
  settingsRows,
  footerNote,
}: {
  leagueName: string;
  subtitle: string;
  meta: string;
  screen: Screen;
  onScreen: (screen: Screen) => void;
  polling: boolean;
  pollHealth: PollHealth | null;
  chime: boolean;
  onToggleChime: () => void;
  onUndo: () => void;
  chatOpen: boolean;
  onToggleChat: () => void;
  settingsOpen: boolean;
  onToggleSettings: () => void;
  settingsRows: SettingsRow[];
  footerNote: string;
}) {
  const menuRef = useRef<HTMLDivElement>(null);

  // A menu that only closes via its own button is a trap on a desktop app.
  useEffect(() => {
    if (!settingsOpen) return undefined;
    const onDown = (event: MouseEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) onToggleSettings();
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onToggleSettings();
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [settingsOpen, onToggleSettings]);

  return (
    <header className="app-header">
      <div className="header-identity">
        <div className="header-title">
          <h1 className="ellipsis">{leagueName}</h1>
          <span className="muted header-subtitle">{subtitle}</span>
        </div>
        <div className="header-modes">
          <div className="mode-toggle" role="group" aria-label="Screen">
            <button
              type="button"
              className={screen === "season" ? "mode is-on" : "mode"}
              onClick={() => onScreen("season")}
              aria-pressed={screen === "season"}
            >
              Season
            </button>
            <button
              type="button"
              className={screen === "draft" ? "mode is-on" : "mode"}
              onClick={() => onScreen("draft")}
              aria-pressed={screen === "draft"}
            >
              Draft
            </button>
          </div>
          <span className="muted header-meta">{meta}</span>
        </div>
      </div>

      <div className="header-actions" ref={menuRef}>
        <SyncPill polling={polling} health={pollHealth} />

        {screen === "draft" && (
          <>
            <button
              type="button"
              className="btn-ghost btn-icon"
              onClick={onUndo}
              title="Undo last recorded pick"
            >
              <UndoIcon />
              <span>Undo</span>
            </button>
            <button
              type="button"
              className={`btn-ghost btn-square${chime ? " is-on" : ""}`}
              onClick={onToggleChime}
              title={chime ? "Pick chime on — click to mute" : "Pick chime muted"}
              aria-pressed={chime}
            >
              <ChimeIcon on={chime} />
            </button>
          </>
        )}

        <button
          type="button"
          className={chatOpen ? "btn-ask is-open" : "btn-ask"}
          onClick={onToggleChat}
          aria-pressed={chatOpen}
        >
          Ask Claude
        </button>

        <button
          type="button"
          className={`btn-ghost btn-square${settingsOpen ? " is-on" : ""}`}
          onClick={onToggleSettings}
          title="Settings"
          aria-expanded={settingsOpen}
        >
          <GearIcon />
        </button>

        {settingsOpen && (
          <div className="settings-menu">
            <div className="settings-menu-head">
              <span className="eyebrow">Settings</span>
              <button type="button" className="link-btn" onClick={onToggleSettings}>
                Done
              </button>
            </div>
            {settingsRows.map((row) => (
              <button key={row.label} type="button" className="settings-row" onClick={row.onSelect}>
                <span className="settings-row-text">
                  <span className="settings-row-label">{row.label}</span>
                  <span className="muted settings-row-note">{row.note}</span>
                </span>
                <span className={row.on ? "settings-row-value is-on" : "settings-row-value"}>
                  {row.value}
                </span>
              </button>
            ))}
            <span className="muted settings-footer">{footerNote}</span>
          </div>
        )}
      </div>
    </header>
  );
}
