// Small display helpers shared across components.

export function fmt(n: number | null | undefined, digits = 0): string {
  if (n === null || n === undefined || Number.isNaN(n)) return "\u2013";
  return n.toFixed(digits);
}

export function pct(p: number | null): string {
  if (p === null) return "\u2013";
  return `${Math.round(p * 100)}%`;
}
