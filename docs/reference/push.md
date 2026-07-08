# Push Notifications

Pulpo's Web Push implementation follows the standard [Web Push
protocol](https://datatracker.ietf.org/doc/html/rfc8030) — VAPID
([RFC 8292](https://datatracker.ietf.org/doc/html/rfc8292)) for sender
identification and ECE ([RFC 8291](https://datatracker.ietf.org/doc/html/rfc8291))
for payload encryption — sent directly from `pulpod` to the browser's push
service. There is no third-party relay: **any spec-compliant client** can
subscribe, not just pulpo's own web UI.

## Subscribing

1. `GET /api/v1/push/vapid-key` — returns the daemon's VAPID public key
   (`{ "public_key": "<base64url>" }`). Auto-generated on first run and stored
   in `[notifications.vapid]` (see [Config Reference](/reference/config)).
2. Create a push subscription with the browser's `PushManager`, using that key
   as the `applicationServerKey`.
3. `POST /api/v1/push/subscribe` with the subscription's endpoint and keys:

```json
{
  "endpoint": "https://fcm.googleapis.com/fcm/send/...",
  "keys": {
    "p256dh": "<base64url>",
    "auth": "<base64url>"
  }
}
```

4. `POST /api/v1/push/unsubscribe` with `{ "endpoint": "..." }` to remove a
   subscription (e.g. before calling `PushSubscription.unsubscribe()`).

Every stored subscription receives every push pulpod sends — there's no
per-subscription event filtering (unlike `[[webhooks]]`, which filter by
`events` glob and `min_severity`). If you don't want push notifications,
don't subscribe.

## What gets pushed

Three canonical [event types](/reference/config#webhooks) are delivered as
push notifications:

| Event type     | When it fires                                             | Carries a "Stop session" action? |
|-----------------|-----------------------------------------------------------|-----------------------------------|
| `lifecycle`     | A session's status changes (active, ready, stopped, lost) | No                                |
| `usage_alert`   | A budget/burn-rate alert fires (typically the 80% warning, *before* the watchdog would auto-stop) | Yes                |
| `intervention`  | The watchdog already forcibly stopped a session            | No — nothing left to action       |

`usage_alert` is the only type with an action button: it's the
"intervention-imminent" moment where stopping the session manually still
matters. By the time an `intervention` event fires, pulpod has already stopped
the session, so there's nothing left to do.

## Payload schema

The JSON body of the push message (the payload passed to your `push` event
handler, `event.data.json()`):

```jsonc
{
  "title": "Budget alert: fix-auth",
  "body": "fix-auth at 82% ($8.20/$10.00)",
  "url": "/sessions/<session-id>",
  "icon": "/icon-192.png",
  "status": "budget_threshold",       // the event's subtype
  "session_id": "<session-id>",
  "session_name": "fix-auth",
  "node_name": "mac-mini",

  // Present only on `usage_alert` payloads, and only when the daemon has a
  // configured action secret (auto-generated on first run — see below):
  "action": {
    "token": "<base64url-json>.<hex-hmac>",
    "label": "Stop session"
  }
}
```

`title`/`body` are derived per event type (budget threshold shows a percentage
and cost/budget; burn-rate shows the cost accrued so far; interventions show
the daemon's recorded reason). This is intentionally a plain, stable JSON
shape — no canonical-envelope wrapping — so any push client can render a
notification without understanding pulpo's internal event model.

Note: pulpo's own service worker (`src/sw.ts`) doesn't read the `icon` field —
it always shows its own app icon. The field is part of the payload contract
for other clients that may want it.

## The action token (`POST /api/v1/push/action`)

A service worker cannot read the web UI's bearer auth token — it only has
whatever the push payload itself carries. So the "Stop session" button
doesn't reuse the app's auth: the `action.token` field is a short-lived,
self-contained capability, valid for **30 minutes**, that authorizes exactly
one thing: stopping the one session it was issued for.

Format: `<base64url(JSON claims)>.<hex HMAC-SHA256 signature>` — the claims
are `{ "session_id": "...", "action": "stop", "exp": <unix-seconds> }`, signed
with a 256-bit secret pulpod generates on first run and stores in
`[notifications.vapid].action_secret` (alongside the VAPID keys, using the
same auto-generate-once-and-persist pattern — never exposed over any API).

To act on it:

```
POST /api/v1/push/action
Content-Type: application/json

{ "token": "<the action.token value from the payload>" }
```

This endpoint is deliberately **unauthenticated** — no bearer token required,
even with `bind = "public"` — because the token itself is the capability. It
responds:

| Status | Meaning |
|--------|---------|
| `200`  | The token verified and the session was stopped. Body: `{ "session_id": "...", "session_name": "..." }`. Also returned if the session was *already* stopped (e.g. the watchdog's own 100% auto-stop beat you to it) — stopping an already-stopped session is a no-op, not an error. |
| `401`  | The token is missing, malformed, tampered, expired, or signed for a different action. One generic message for every case — a bad token can't be used as an oracle to learn *why* it failed. |
| `410`  | The token itself verified fine, but its target session no longer exists (e.g. purged via `?purge=true` or `pulpo cleanup`). Distinct from a `404`: the *token* was valid, only its target is gone. |

Pulpo's own web app never calls this endpoint directly — only the service
worker's `notificationclick` handler does, in response to the user tapping
the action button.

## Service worker requirements

- **Chrome, Firefox, Edge (desktop and Android)**: push notifications work
  once the page is served over HTTPS (or `localhost`) and permission is
  granted — no special setup.
- **iOS/iPadOS Safari**: Web Push only works from a PWA that has been
  **Added to Home Screen** (A2HS) — Safari does not deliver push to an
  ordinary browser tab. Add pulpo's web UI to the home screen first, then
  enable push from inside that installed app.
- The service worker derives the daemon's origin from
  `self.registration.scope` when calling `/api/v1/push/action` — since pulpo's
  web UI is served by the same `pulpod` it talks to, no separate base URL
  needs to travel in the push payload.

## See also

- [Configuration Guide § Notifications](/guides/configuration#notifications) — webhooks vs. push
- [Config Reference § `[notifications.vapid]`](/reference/config#notifications-vapid) — key/secret fields
- [API Reference § Push Notifications](/reference/api) — endpoint list
