// Theme resolution: follow the OS by default, remember an explicit override.
// The preference is one of the remembered choices in persisted.ts, so reading
// it, storing it and applying it all live here rather than in App.tsx.

import { useEffect } from "react";
import { persisted, usePersisted } from "./persisted";

export type Theme = "light" | "dark";
export type ThemePreference = Theme | "system";

const preferenceStore = persisted<ThemePreference>(
  "da.theme",
  (raw) => (raw === "light" || raw === "dark" || raw === "system" ? raw : null),
  "system",
);

function media(): MediaQueryList | null {
  return typeof window.matchMedia === "function"
    ? window.matchMedia("(prefers-color-scheme: dark)")
    : null;
}

export function systemTheme(): Theme {
  return media()?.matches ? "dark" : "light";
}

export function resolveTheme(preference: ThemePreference): Theme {
  return preference === "system" ? systemTheme() : preference;
}

export function applyTheme(theme: Theme): void {
  document.documentElement.dataset.theme = theme;
}

/** Step through system -> light -> dark -> system, the settings row's job. */
export function cycleThemePreference(): void {
  const current = preferenceStore.get();
  preferenceStore.set(current === "system" ? "light" : current === "light" ? "dark" : "system");
}

/** Test seam: forget this session's choice and re-read what is stored. */
export function resetThemePreference(): void {
  preferenceStore.reset();
}

/**
 * Watch the OS setting. Only used while the preference is "system" — an
 * explicit choice should not be overridden by the OS flipping.
 */
function watchSystemTheme(onChange: (theme: Theme) => void): () => void {
  const query = media();
  if (query === null) return () => undefined;
  const handler = (event: MediaQueryListEvent) => onChange(event.matches ? "dark" : "light");
  query.addEventListener("change", handler);
  return () => query.removeEventListener("change", handler);
}

/**
 * The preference, kept applied to the page: one hook instead of the state and
 * two effects the shell used to carry.
 */
export function useAppliedTheme(): { preference: ThemePreference; theme: Theme } {
  const chosen = usePersisted(preferenceStore);
  useEffect(() => {
    applyTheme(resolveTheme(chosen));
    if (chosen !== "system") return undefined;
    return watchSystemTheme(applyTheme);
  }, [chosen]);
  return { preference: chosen, theme: resolveTheme(chosen) };
}
