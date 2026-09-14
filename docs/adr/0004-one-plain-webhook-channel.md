# 0004. One plain webhook channel

- **Status:** Accepted
- **Date:** 2026-09-13 (PR [#111](https://github.com/darioblanco/pulpo/pull/111))

## Context

The event-forwarding backbone had grown into a small monitoring platform of its own: a
durable SQLite outbox with retry + exponential backoff, HMAC-SHA256 request signing
with per-endpoint secrets, Web Push (VAPID keys, push subscriptions, a "Stop session"
push action), and a toggleable Prometheus `/metrics` endpoint — on top of the plain
`[[webhooks]]` channel and the SSE `/api/v1/events` stream. A Discord-specific notifier
had already been removed earlier (PR #56) for the same reason: bespoke channel-specific
code next to a general mechanism nobody had asked to extend further. This was a lot of
durability and security machinery for a self-hosted, single-operator daemon whose
webhook receiver is typically the operator's own collector on infrastructure they
already trust.

## Decision

We will collapse notification delivery to **one plain webhook channel**: `[[webhooks]]`
(name/url/`events` glob/`min_severity`), delivered as an unsigned POST with a fixed
retry schedule (~1s/3s/9s) from an in-memory queue. A delivery that exhausts its
retries is logged and dropped, not persisted. (Delivery was bounded further the same
month by PR #118: connect/total timeouts and a cap of 16 concurrent in-flight
deliveries, with the request URL stripped from failure logs since a webhook URL embeds
its own secret.)

Removed in the same PR: the VAPID keys and push-subscription flow, the "Stop session"
push action token, the `/api/v1/push/*` endpoints, the Prometheus `/api/v1/metrics`
endpoint and `[metrics]` config, the SQLite `webhook_outbox` table, and the
per-endpoint `secret` config key (which existed to key the now-removed HMAC signing).

Kept: the canonical event envelope (`event_id`, `schema_version`, `type`, `severity`,
`occurred_at`, `node`, `session_id?`, `payload`), the SSE `/api/v1/events` stream, and
the in-app toast notification path.

## Consequences

- Simpler mental model and a smaller attack surface: one delivery mechanism, no signing
  keys to provision or rotate, no outbox schema to migrate.
- No delivery durability across a `pulpod` restart — an in-flight retry queue is
  memory-only. Accepted tradeoff for a single-node tool where the operator already
  controls both ends of the webhook; a lost alert during a restart window is a
  cheaper failure mode than the outbox/signing complexity was buying.
- No HMAC verification on the receiving end — the receiver must be a system the
  operator already trusts (their own collector, not a public endpoint), documented as
  such in the config reference.
- No built-in Prometheus scraping surface — continuous dashboard state (session counts,
  costs) is no longer pull-based; forward discrete events via webhooks to whatever
  collector the operator runs instead.
- Removing `web-push` also dropped the vendored `openssl`/`p256`/`hmac`/`sha2`/`hex`
  dependencies from the tree, shrinking the build.
