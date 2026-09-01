import "@testing-library/jest-dom/vitest";
import { beforeEach } from "vitest";

// jsdom is configured without web storage here, but chats, preferences and
// the theme are all things the app remembers in it. A plain in-memory stand-in
// gives those a real place to write, emptied between tests so no test inherits
// what another one saved. Tests that need storage to misbehave still stub
// their own over the top of this one.
const saved = new Map<string, string>();
const memoryStorage = {
  getItem: (key: string) => saved.get(key) ?? null,
  setItem: (key: string, value: string) => void saved.set(key, String(value)),
  removeItem: (key: string) => void saved.delete(key),
  clear: () => saved.clear(),
};

// Defined unconditionally: Node exposes a `localStorage` property that is
// undefined unless it was started with a storage file, so "is it there?" is
// not a question a truthiness check can answer.
Object.defineProperty(globalThis, "localStorage", { value: memoryStorage, configurable: true });

beforeEach(() => {
  saved.clear();
});
