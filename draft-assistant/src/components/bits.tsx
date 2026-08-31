// Small shared primitives. Everything visual that appears in more than one
// screen lives here so the two screens can't drift apart.

import { useEffect, useRef, useState } from "react";
import { headshotSrc, teamAvatarSrc, useAvatarMode } from "../avatars";
import { injuryWord, teamLogo } from "../format";
import { closeZoom, openZoom, useZoom, type Zoomed } from "../zoom";
import { useFocusTrap } from "./useFocusTrap";

/** NFL team mark. Renders nothing when the player has no team (free agents). */
export function TeamLogo({
  team,
  className,
}: {
  team: string | null | undefined;
  className?: string;
}) {
  const src = teamLogo(team);
  if (src === null) return null;
  return (
    <img
      className={className === undefined ? "team-logo" : `team-logo ${className}`}
      src={src}
      alt=""
      width={15}
      height={15}
      loading="lazy"
    />
  );
}

/** Wraps a picture in a button so clicking it opens the larger copy. */
function Zoomable({ children, ...zoomed }: Zoomed & { children: React.ReactNode }) {
  return (
    <button
      type="button"
      className="zoomable"
      title={zoomed.label}
      aria-label={`Show a larger picture of ${zoomed.label}`}
      onClick={() => openZoom(zoomed)}
    >
      {children}
    </button>
  );
}

/** The one picture currently zoomed, over everything else. Rendered once. */
export function ZoomLayer() {
  const zoomed = useZoom();
  // Keyed by the reference it belongs to, so a stale answer for the picture
  // you just closed can never be drawn over the one you just opened.
  const [big, setBig] = useState<{ reference: string; url: string | null } | null>(null);
  const wanted = zoomed?.avatar;

  useEffect(() => {
    if (wanted === undefined) return;
    let cancelled = false;
    // Fire-and-forget: teamAvatarSrc already folds every failure into a null
    // url, so this promise cannot reject and there is nothing to await.
    void teamAvatarSrc(wanted, true).then((url) => {
      if (!cancelled) setBig({ reference: wanted, url });
    });
    return () => {
      cancelled = true;
    };
  }, [wanted]);

  // The picture that opened this, so focus can go back where it came from.
  const opener = useRef<HTMLElement | null>(null);
  const closeButton = useRef<HTMLButtonElement>(null);
  const card = useRef<HTMLElement>(null);

  useEffect(() => {
    if (zoomed === null) return;
    opener.current = document.activeElement as HTMLElement | null;
    closeButton.current?.focus();
    return () => {
      // Returning focus to the thumbnail keeps a keyboard user where they
      // were in the table instead of dropping them at the top of the page.
      opener.current?.focus();
      opener.current = null;
    };
  }, [zoomed]);

  // A modal must not leak focus to the page behind it. Close is the only stop
  // in here, so Tab and Shift+Tab both land back on it.
  useFocusTrap(card, closeZoom, zoomed !== null);

  if (zoomed === null) return null;
  return (
    <div
      className="zoom-layer"
      role="dialog"
      aria-modal="true"
      aria-label={zoomed.label}
      onClick={closeZoom}
    >
      <figure className="zoom-card" ref={card} onClick={(e) => e.stopPropagation()}>
        <img
          className="zoom-image"
          src={(big !== null && big.reference === wanted ? big.url : null) ?? zoomed.src}
          alt={zoomed.label}
        />
        <figcaption>{zoomed.label}</figcaption>
        <button type="button" className="zoom-close" ref={closeButton} onClick={closeZoom}>
          Close
        </button>
      </figure>
    </div>
  );
}

/** Coloured position label, optionally with its positional rank appended. */
export function PosBadge({ position, rank }: { position: string; rank?: number | null }) {
  return (
    <span className={`pos-badge pos-${position}`}>
      {position}
      {rank !== undefined && rank !== null && <span className="pos-rank">{rank}</span>}
    </span>
  );
}

/** Player name preceded by their team mark, clipped rather than wrapped. */
/** Sleeper's player thumbnail, falling back to the team logo when there is
 * none (defences, unknown players, or an image that fails to load). */
export function Headshot({
  playerId,
  team,
  name,
}: {
  playerId: string | null | undefined;
  team: string | null | undefined;
  /** Caption for the zoomed view; the row's own name text. */
  name?: string;
}) {
  const mode = useAvatarMode();
  const isPlayer = typeof playerId === "string" && /^\d+$/.test(playerId);
  const wanted = mode === "headshots" && isPlayer ? playerId : null;
  // A defence's id is its team code ("JAX"), which is also the only mark it
  // has — callers that know no separate team still get a logo.
  const fallbackTeam = team ?? (typeof playerId === "string" && !isPlayer ? playerId : null);
  const [src, setSrc] = useState<{ id: string | null; url: string | null }>({
    id: null,
    url: null,
  });

  // Resolved through the session cache, which sits in front of the
  // backend's on-disk copy: Sleeper is asked for each face once, ever.
  useEffect(() => {
    if (wanted === null) return;
    let cancelled = false;
    // Fire-and-forget: headshotSrc already folds every failure into a null
    // url, so this promise cannot reject and there is nothing to await.
    void headshotSrc(wanted).then((url) => {
      if (!cancelled) setSrc({ id: wanted, url });
    });
    return () => {
      cancelled = true;
    };
  }, [wanted]);

  const url = wanted !== null && src.id === wanted ? src.url : null;
  // Every branch fills the same round slot, so switching between photos and
  // team marks (or hitting a player with neither) never nudges the row.
  const caption = name ?? fallbackTeam ?? "player";
  if (url === null) {
    const logo = teamLogo(fallbackTeam);
    if (logo === null) return <span className="avatar avatar-blank" aria-hidden="true" />;
    return (
      <Zoomable src={logo} label={caption}>
        <TeamLogo team={fallbackTeam} className="avatar avatar-logo" />
      </Zoomable>
    );
  }
  return (
    <Zoomable src={url} label={caption}>
      <img
        className="avatar headshot"
        src={url}
        alt=""
        width={22}
        height={22}
        onError={() => setSrc({ id: wanted, url: null })}
      />
    </Zoomable>
  );
}

/** A manager's team picture. Renders the team's initial while it loads and
 * when they never set one, so the row's shape never depends on the network. */
export function TeamAvatar({ avatar, name }: { avatar?: string | null; name: string }) {
  const [src, setSrc] = useState<string | null>(null);

  useEffect(() => {
    if (avatar === null || avatar === undefined) return;
    let cancelled = false;
    // Fire-and-forget: teamAvatarSrc already folds every failure into a null
    // url, so this promise cannot reject and there is nothing to await.
    void teamAvatarSrc(avatar).then((url) => {
      if (!cancelled) setSrc(url);
    });
    return () => {
      cancelled = true;
    };
  }, [avatar]);

  if (src === null) {
    return (
      <span className="team-avatar is-blank" aria-hidden="true">
        {name.trim().charAt(0).toUpperCase() || "?"}
      </span>
    );
  }
  return (
    <Zoomable src={src} label={name} avatar={avatar ?? undefined}>
      <img className="team-avatar" src={src} alt="" width={18} height={18} />
    </Zoomable>
  );
}

export function PlayerName({
  name,
  team,
  tag,
  tagTitle,
  playerId,
}: {
  name: string;
  team: string | null | undefined;
  tag?: string | null;
  /** Spelled-out form of a short tag: "O" -> "Out". Worked out from the tag
   * itself when the caller does not pass one. */
  tagTitle?: string;
  /** When given, shows the player's headshot instead of the team logo. */
  playerId?: string | null;
}) {
  // A player who is Out or Doubtful is the one worth colouring; Questionable
  // is common enough that shouting about it would be noise.
  const alarming = tag === "Out" || tag === "O" || tag === "D";
  // "Q" means nothing read out on its own, and hovering is not an option for
  // everyone, so the whole word goes in the page for screen readers while the
  // badge stays one letter wide on screen.
  const spelled = tag ? (tagTitle ?? injuryWord(tag)) : null;
  return (
    <span className="player-name">
      {playerId === undefined ? (
        <TeamLogo team={team} />
      ) : (
        <Headshot playerId={playerId} team={team} name={name} />
      )}
      <span className="ellipsis">{name}</span>
      {tag && (
        <span className={alarming ? "tag tag-out" : "tag"} title={spelled ?? undefined}>
          {spelled === tag ? (
            tag
          ) : (
            <>
              <span aria-hidden="true">{tag}</span>
              <span className="sr-only">{spelled}</span>
            </>
          )}
        </span>
      )}
    </span>
  );
}

/** Section heading used above every panel. */
export function PanelHead({ title, note }: { title: string; note?: React.ReactNode }) {
  return (
    <div className="panel-head">
      <span className="eyebrow">{title}</span>
      {note !== undefined && <span className="panel-head-note">{note}</span>}
    </div>
  );
}

/** Sortable column header with its active-direction arrow. */
export function SortHead({
  label,
  active,
  direction,
  align,
  onClick,
  title,
}: {
  label: string;
  active: boolean;
  direction: "asc" | "desc";
  align?: "right";
  onClick: () => void;
  title?: string;
}) {
  // `aria-sort` belongs on a columnheader cell, and these boards are CSS
  // grids of buttons rather than tables — set there it announces nothing at
  // all. Putting the state in the accessible name is what actually reaches a
  // screen reader, and it keeps the arrow purely decorative.
  const sortState = active
    ? direction === "asc"
      ? "sorted ascending"
      : "sorted descending"
    : "not sorted";
  return (
    <button
      type="button"
      className={`sort-head${active ? " is-active" : ""}${align === "right" ? " is-right" : ""}`}
      onClick={onClick}
      title={title}
      aria-label={`${label}, ${sortState}`}
    >
      {label}
      <span className="sort-arrow" aria-hidden="true">
        {active ? (direction === "asc" ? "▲" : "▼") : " "}
      </span>
    </button>
  );
}

/** Segmented control: one button per option, the active one filled. */
export function Segmented<T extends string>({
  options,
  value,
  onChange,
  titles,
  label,
}: {
  options: readonly T[];
  value: T;
  onChange: (next: T) => void;
  titles?: Partial<Record<T, string>>;
  label: string;
}) {
  return (
    <div className="segmented" role="group" aria-label={label}>
      {options.map((option) => (
        <button
          key={option}
          type="button"
          className={option === value ? "seg is-on" : "seg"}
          onClick={() => onChange(option)}
          title={titles?.[option]}
          aria-pressed={option === value}
        >
          {option}
        </button>
      ))}
    </div>
  );
}

/** The empty state used when a panel has nothing to show. */
export function Empty({ children }: { children: React.ReactNode }) {
  return <p className="empty-note">{children}</p>;
}
