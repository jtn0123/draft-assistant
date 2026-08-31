import { act } from "@testing-library/react";

/**
 * Run a synchronous interaction and let everything it scheduled finish.
 *
 * React Testing Library only returns something awaitable from `act` when the
 * scope itself is async, so flushing the effects a click or a key press queues
 * means draining the microtask queue from inside the scope. Wrapped here once
 * rather than repeated as a bare `await act(async () => …)` in every test.
 */
export async function settle(interact: () => void): Promise<void> {
  await act(async () => {
    interact();
    await Promise.resolve();
  });
}
