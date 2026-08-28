import "@testing-library/jest-dom/vitest";

// jsdom has no <dialog> behaviour. Enough of it for the confirm dialog: the
// `open` attribute, and a `close` event so the component's close handling runs.
const dialogProto = Object.getPrototypeOf(document.createElement("dialog")) as HTMLDialogElement;
if (typeof dialogProto.showModal !== "function") {
  dialogProto.showModal = function (this: HTMLDialogElement) {
    this.setAttribute("open", "");
  };
  dialogProto.show = dialogProto.showModal;
  dialogProto.close = function (this: HTMLDialogElement) {
    this.removeAttribute("open");
    this.dispatchEvent(new Event("close"));
  };
}

// This jsdom build exposes no Web Storage. The chat panel remembers its
// settings there, so give it an in-memory one.
if (typeof window.localStorage === "undefined") {
  const store = new Map<string, string>();
  const memoryStorage: Storage = {
    get length() {
      return store.size;
    },
    clear: () => store.clear(),
    getItem: (key) => store.get(key) ?? null,
    key: (index) => [...store.keys()][index] ?? null,
    removeItem: (key) => {
      store.delete(key);
    },
    setItem: (key, value) => {
      store.set(key, String(value));
    },
  };
  Object.defineProperty(window, "localStorage", { value: memoryStorage, configurable: true });
}
