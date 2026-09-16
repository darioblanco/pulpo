// Stale service-worker kill switch.
//
// Pulpo's web UI has never intentionally registered a service worker, but the
// static file handler used to fall back to the SPA (`index.html`) for any
// unknown path — including `/sw.js` — so a browser that somehow picked one up
// (a stray registration from local dev, a misconfigured proxy, etc.) would
// keep re-installing and serving a stale cached bundle forever, with no way
// for the daemon to reach it.
//
// Shipping this file at `/sw.js` lets the daemon serve a *real* worker whose
// only job is to immediately unregister itself, clear any caches it created,
// and reload any open tabs so they go back to fetching straight from the
// network. Once installed, it removes itself — it does not keep running.
//
// Safe to delete once v0.6.0 ships (two releases past its introduction in
// v0.4.x): by then any browser that ever had a stale worker installed will
// have long since picked up this kill switch and unregistered it.

self.addEventListener('install', () => {
  // Don't wait for old clients to close — activate right away so the
  // kill switch runs as soon as possible.
  self.skipWaiting();
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    (async () => {
      // Remove any caches a previous (unknown) service worker may have created.
      const cacheKeys = await caches.keys();
      await Promise.all(cacheKeys.map((key) => caches.delete(key)));

      // Unregister so this worker doesn't keep intercepting future requests.
      await self.registration.unregister();

      // Force every open tab back onto the network instead of whatever the
      // old worker was serving from cache.
      const clientList = await self.clients.matchAll({ type: 'window' });
      for (const client of clientList) {
        if ('navigate' in client) {
          client.navigate(client.url);
        } else {
          client.postMessage({ type: 'RELOAD' });
        }
      }
    })(),
  );
});
