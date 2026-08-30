// Theme resolution: follow the OS by default, remember an explicit override.

export type Theme = "light" | "dark";
export type ThemePreference = Theme | "system";

const STORAGE_KEY = "da.theme";

function media(): MediaQueryList | null {
  return typeof window.matchMedia === "function"
    ? window.matchMedia("(prefers-color-scheme: dark)")
    : null;
}

export function systemTheme(): Theme {
  return media()?.matches ? "dark" : "light";
}

/** The stored preference, or "system" when the user has never chosen. */
export function storedPreference(): ThemePreference {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === "light" || raw === "dark" || raw === "system") return raw;
  } catch {
    // Private browsing and locked-down webviews both throw here.
  }
  return "system";
}

export function savePreference(preference: ThemePreference): void {
  try {
    localStorage.setItem(STORAGE_KEY, preference);
  } catch {
    // Not being able to persist is not a reason to refuse the change.
  }
}

export function resolveTheme(preference: ThemePreference): Theme {
  return preference === "system" ? systemTheme() : preference;
}

export function applyTheme(theme: Theme): void {
  document.documentElement.dataset.theme = theme;
}

/**
 * Watch the OS setting. The callback only fires while the preference is
 * "system" — an explicit choice should not be overridden by the OS flipping.
 */
export function watchSystemTheme(onChange: (theme: Theme) => void): () => void {
  const query = media();
  if (query === null) return () => undefined;
  const handler = (event: MediaQueryListEvent) => onChange(event.matches ? "dark" : "light");
  query.addEventListener("change", handler);
  return () => query.removeEventListener("change", handler);
}
