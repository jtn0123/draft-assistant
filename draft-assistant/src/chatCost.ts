// Display estimated API-equivalent usage cost supplied by the backend.

/** "$0.42", or "$0.004" while a conversation is still worth less than a cent. */
export function formatUsd(amount: number): string {
  if (amount > 0 && amount < 0.01) return `$${amount.toFixed(3)}`;
  return `$${amount.toFixed(2)}`;
}
