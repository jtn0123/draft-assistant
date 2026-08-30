/// <reference types="vite/client" />

declare global {
  interface Window {
    /** Safari's prefixed AudioContext, used as a fallback for the pick chime. */
    webkitAudioContext?: typeof AudioContext;
  }
}

export {};
