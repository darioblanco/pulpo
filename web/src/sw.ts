/// <reference lib="webworker" />
declare const self: ServiceWorkerGlobalScope;

import { precacheAndRoute, createHandlerBoundToURL } from 'workbox-precaching';
import { NavigationRoute, registerRoute } from 'workbox-routing';

// Precache static assets (JS, CSS, images)
precacheAndRoute(self.__WB_MANIFEST);

// SPA navigation fallback — serve index.html for all navigation requests
// except API calls, events, and health endpoints
const handler = createHandlerBoundToURL('/index.html');
const navigationRoute = new NavigationRoute(handler, {
  denylist: [/^\/api\//, /^\/events/],
});
registerRoute(navigationRoute);

// Activate immediately — don't wait for old tabs to close
self.addEventListener('install', () => {
  self.skipWaiting();
});
self.addEventListener('activate', (event) => {
  event.waitUntil(self.clients.claim());
});
