// Small display helpers shared across components.

export function fmt(n: number | null | undefined, digits = 0): string {
  if (n === null || n === undefined || Number.isNaN(n)) return "–";
  return n.toFixed(digits);
}

export function pct(p: number | null): string {
  if (p === null) return "–";
  return `${Math.round(p * 100)}%`;
}

/**
 * The message of anything a command or fetch can reject with — a bare string
 * from a Rust `Err(String)`, or an Error from the frontend — without the
 * "Error: " prefix that String(error) would add.
 */
export function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}
