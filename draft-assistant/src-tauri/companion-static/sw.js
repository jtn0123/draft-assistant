/* Draft Assistant companion service worker.
   Here only so the page can be installed to a phone's home screen, which the
   browser will not offer without one. It caches nothing: everything the page
   shows is live from the host, and a cached board is a wrong board. Every
   request goes straight through to the network. */
self.addEventListener("install", () => {
  self.skipWaiting();
});
self.addEventListener("activate", (event) => {
  event.waitUntil(self.clients.claim());
});
self.addEventListener("fetch", (event) => {
  event.respondWith(fetch(event.request));
});
