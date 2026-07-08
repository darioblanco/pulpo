/// <reference lib="webworker" />
declare const self: ServiceWorkerGlobalScope;

import { precacheAndRoute, createHandlerBoundToURL } from 'workbox-precaching';
import { NavigationRoute, registerRoute } from 'workbox-routing';
import {
  STOP_ACTION,
  buildNotificationOptions,
  buildStopResultNotification,
  postStopAction,
  type PushPayload,
} from './lib/push-sw';

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

// Web Push notification handling
self.addEventListener('push', (event) => {
  const data: PushPayload = event.data?.json() ?? {};
  const { title, options } = buildNotificationOptions(data);
  event.waitUntil(self.registration.showNotification(title, options));
});

/** Focus an existing tab on `url`, or open a new one. */
async function focusOrOpenWindow(url: string): Promise<void> {
  const clients = await self.clients.matchAll({ type: 'window', includeUncontrolled: true });
  for (const client of clients) {
    if (new URL(client.url).pathname === url && 'focus' in client) {
      await client.focus();
      return;
    }
  }
  await self.clients.openWindow(url);
}

/** POST the "Stop session" action token, then show a confirmation notification. */
async function handleStopAction(actionToken: string): Promise<void> {
  const result = await postStopAction(self.registration.scope, actionToken);
  const { title, options } = buildStopResultNotification(result);
  await self.registration.showNotification(title, options);
}

self.addEventListener('notificationclick', (event) => {
  const { action, notification } = event;
  notification.close();

  const actionToken = notification.data?.actionToken as string | undefined;
  if (action === STOP_ACTION && actionToken) {
    event.waitUntil(handleStopAction(actionToken));
    return;
  }

  event.waitUntil(focusOrOpenWindow(notification.data?.url ?? '/'));
});
