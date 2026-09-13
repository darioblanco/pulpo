# Example: Pulpo webhook → Discord

A tiny, dependency-free reference for **building your own integration** on Pulpo's
universal webhook. It receives Pulpo's canonical event envelope, de-duplicates retries,
filters by severity, and posts a message to a Discord webhook.

Pulpo deliberately has no built-in Discord notifier — instead it forwards every event
(session lifecycle, interventions, usage/cost alerts, fleet health) to any HTTP endpoint
you configure. This example is the pattern for consuming that: swap the Discord-posting bit
for Slack, PagerDuty, ntfy, an internal collector, or whatever you run.

## The message Pulpo sends

```
POST <your endpoint>
  Content-Type: application/json
  User-Agent: pulpo/<version>
  X-Pulpo-Event: lifecycle.idle          # "<type>.<subtype>" — route/drop without parsing
  X-Pulpo-Event-Id: <uuid>               # stable across retries of one delivery attempt

{
  "schema_version": 1,
  "event_id": "<uuid>",
  "type": "lifecycle",          // lifecycle | intervention | usage_alert | fleet
  "subtype": "idle",
  "severity": "warn",           // info | warn | critical
  "occurred_at": "2026-06-13T12:00:00Z",
  "node": "mac-mini",
  "session": {                  // present for session-scoped events
    "id": "...", "name": "fix-auth", "status": "idle",
    "git_branch": "...", "pr_url": null,
    "cost_usd": 2.5, "total_tokens": 1234000, "pool": "subscription"
  },
  "payload": { }                // type-specific extras (budget_usd, quota_used_percent, ...)
}
```

Delivery is a **plain POST** (no request signing — treat the URL itself as the shared
secret, or put this endpoint behind your own auth) from an in-memory queue: the initial
attempt plus up to 3 retries (~1s, 3s, 9s), then Pulpo logs the failure and drops the
event — there's no durable outbox, so nothing is retried after that. A receiver should
still:

1. **De-duplicate** on `X-Pulpo-Event-Id` (a retry of the same delivery reuses the id —
   this example keeps an in-memory set; use a real store in production).
2. Return any **2xx** to acknowledge; a non-2xx triggers Pulpo's next retry, if any remain.

## Run it

```bash
cd contrib/examples/webhook-discord
cp .env.example .env   # then edit
node --env-file=.env index.mjs     # Node 20+ (built-in fetch, http; no deps)
```

Then point a Pulpo webhook at it:

```toml
# ~/.pulpo/config.toml
[[webhooks]]
name = "discord-relay"                  # required — unique endpoint id
url = "http://localhost:8099/pulpo"
events = ["lifecycle.*", "usage_alert.*", "intervention.*"]  # globs on "<type>.<subtype>"
min_severity = "warn"                   # info < warn < critical
```

## Environment

| Var | Required | Default | Description |
|-----|----------|---------|-------------|
| `DISCORD_WEBHOOK_URL` | yes | — | Your Discord channel webhook URL |
| `PORT` | no | `8099` | Port to listen on |
| `MIN_SEVERITY` | no | `info` | Drop events below this (`info` < `warn` < `critical`) |
