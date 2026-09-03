// League header: identity, Draft/Season toggle, sync status, and the
// settings menu that hangs off it.

import { useEffect, useRef } from "react";
import type { PollHealth } from "../types";
import { age } from "../format";
import { setChime, useChime, type Screen } from "../prefs";

export interface SettingsRow {
  label: string;
  note: string;
  value: string;
  on: boolean;
  onSelect: () => void;
}

function SyncPill({
  polling,
  health,
  screen,
}: {
  polling: boolean;
  health: PollHealth | null;
  screen: Screen;
}) {
  const failures = health?.consecutive_failures ?? 0;
  const detail = health?.last_error ?? null;
  if (!polling) {
    // The season screen already has a DATA badge that says "Not updating" a
    // few centimetres away. Two words for one state read as two states, so
    // this pill borrows that one rather than introducing "sync" beside it.
    return (
      <span className="pill pill-off">
        <span className="dot" />
        {screen === "season" ? "Not updating" : "Live sync off"}
      </span>
    );
  }
  if (failures >= 1) {
    // Why sync is failing used to live in a tooltip on a span nobody could
    // reach with a keyboard — the only place the reason appeared at all. It
    // is written under the pill now, where everyone can read it.
    return (
      <span className="sync-status">
        <span className="pill pill-stale" title={detail ?? undefined}>
          <span className="dot" />
          {failures >= 2 ? `Sync stale · ${failures} failures` : "Sync retrying"}
        </span>
        {detail !== null && <span className="muted sync-detail">Last try failed: {detail}</span>}
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
  onSwitchLeague,
  subtitle,
  meta,
  screen,
  onScreen,
  polling,
  pollHealth,
  onUndo,
  chatOpen,
  onToggleChat,
  settingsOpen,
  onToggleSettings,
  settingsRows,
  footerNote,
}: {
  leagueName: string;
  /** Opens the league picker; the name in the header is the way in. */
  onSwitchLeague: () => void;
  subtitle: string;
  meta: string;
  screen: Screen;
  onScreen: (screen: Screen) => void;
  polling: boolean;
  pollHealth: PollHealth | null;
  onUndo: () => void;
  chatOpen: boolean;
  onToggleChat: () => void;
  settingsOpen: boolean;
  onToggleSettings: () => void;
  settingsRows: SettingsRow[];
  footerNote: string;
}) {
  // The chime is a preference the header owns outright: it reads it from the
  // store and flips it there, rather than being handed both halves as props.
  const chime = useChime();
  // Wraps the gear and the menu together, so focus moving between the two
  // does not read as leaving.
  const menuRef = useRef<HTMLDivElement>(null);
  const menuBox = useRef<HTMLDivElement>(null);
  const gearRef = useRef<HTMLButtonElement>(null);
  const firstRow = useRef<HTMLButtonElement>(null);
  // Closing usually means handing focus back to the gear. Tabbing or clicking
  // away is the exception: the user has already chosen where to go next.
  const returnFocus = useRef(true);
  const wasOpen = useRef(false);

  // A menu that only closes via its own button is a trap on a desktop app.
  useEffect(() => {
    if (!settingsOpen) return undefined;
    const close = (restore: boolean) => {
      returnFocus.current = restore;
      onToggleSettings();
    };
    const onDown = (event: MouseEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) close(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") close(true);
    };
    // Tabbing past the last row used to leave the menu open behind the user.
    const onFocusOut = (event: FocusEvent) => {
      const next = event.relatedTarget as Node | null;
      if (next !== null && menuRef.current?.contains(next) !== true) close(false);
    };
    const box = menuRef.current;
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    box?.addEventListener("focusout", onFocusOut);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
      box?.removeEventListener("focusout", onFocusOut);
    };
  }, [settingsOpen, onToggleSettings]);

  // Opening puts the keyboard on the first setting; closing puts it back on
  // the gear, so nobody has to Tab their way home from the top of the page.
  useEffect(() => {
    if (settingsOpen) {
      wasOpen.current = true;
      firstRow.current?.focus();
      return;
    }
    if (wasOpen.current && returnFocus.current) gearRef.current?.focus();
    wasOpen.current = false;
    returnFocus.current = true;
  }, [settingsOpen]);

  // Up and down walk the menu, Home and End jump to its ends — the moves the
  // menu role already promises. Every item sits at tabIndex -1, so Tab leaves
  // the menu (and closes it) rather than crawling through six settings.
  const onMenuKey = (event: React.KeyboardEvent) => {
    const items = [
      ...(menuBox.current?.querySelectorAll<HTMLElement>(
        '[role="menuitem"], [role="menuitemcheckbox"]',
      ) ?? []),
    ];
    if (items.length === 0) return;
    const at = items.indexOf(document.activeElement as HTMLElement);
    const step = event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0;
    let next = -1;
    if (step !== 0) {
      next =
        at < 0 ? (step === 1 ? 0 : items.length - 1) : (at + step + items.length) % items.length;
    } else if (event.key === "Home") {
      next = 0;
    } else if (event.key === "End") {
      next = items.length - 1;
    }
    if (next < 0) return;
    event.preventDefault();
    items[next]?.focus();
  };

  return (
    <header className="app-header">
      <div className="header-identity">
        <div className="header-title">
          <h1 className="ellipsis">
            <button
              type="button"
              className="league-switch"
              title="Switch league"
              onClick={onSwitchLeague}
            >
              {leagueName}
              <svg className="league-switch-caret" viewBox="0 0 10 10" aria-hidden="true">
                <path d="M2 3.5l3 3 3-3" fill="none" stroke="currentColor" strokeWidth="1.5" />
              </svg>
            </button>
          </h1>
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
        <SyncPill polling={polling} health={pollHealth} screen={screen} />

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
              onClick={() => setChime(!chime)}
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
          ref={gearRef}
          aria-haspopup="menu"
          aria-expanded={settingsOpen}
        >
          <GearIcon />
        </button>

        {settingsOpen && (
          <div className="settings-menu" role="menu" aria-label="Settings" ref={menuBox}>
            <div className="settings-menu-head" role="none">
              <span className="eyebrow">Settings</span>
              <button
                type="button"
                className="link-btn"
                role="menuitem"
                tabIndex={-1}
                onClick={onToggleSettings}
                // Arrows move between items, so the handler sits on the items
                // themselves: the menu box around them never holds focus.
                onKeyDown={onMenuKey}
              >
                Done
              </button>
            </div>
            {settingsRows.map((row, index) => (
              <button
                key={row.label}
                type="button"
                className="settings-row"
                // The row's setting is its state, not a word in its label: a
                // screen reader should say "on", not read "On" as part of the
                // name and leave the listener to guess it was a control.
                role="menuitemcheckbox"
                aria-checked={row.on}
                tabIndex={-1}
                ref={index === 0 ? firstRow : undefined}
                onClick={row.onSelect}
                onKeyDown={onMenuKey}
              >
                <span className="settings-row-text">
                  <span className="settings-row-label">{row.label}</span>
                  <span className="muted settings-row-note">{row.note}</span>
                </span>
                <span className={row.on ? "settings-row-value is-on" : "settings-row-value"}>
                  {row.value}
                </span>
              </button>
            ))}
            <span className="muted settings-footer" role="none">
              {footerNote}
            </span>
          </div>
        )}
      </div>
    </header>
  );
}
