-- Durable outbox for webhook delivery.
--
-- Each row is one (endpoint, event) delivery attempt that survives daemon
-- restarts. The dispatcher enqueues a pending row instead of POSTing inline; a
-- delivery worker drains due rows with retry + exponential backoff. The stored
-- `envelope_json` is the exact body that gets POSTed on every retry, so the
-- receiver dedupes on the stable `event_id` (idempotency key).
CREATE TABLE webhook_outbox (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    endpoint TEXT NOT NULL,            -- WebhookEndpointConfig.name
    event_id TEXT NOT NULL,            -- envelope event_id (receiver dedupe key)
    envelope_json TEXT NOT NULL,       -- exact serialized canonical Event posted
    status TEXT NOT NULL DEFAULT 'pending',  -- pending | delivered | dead
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TEXT NOT NULL,     -- rfc3339; row is due when <= now
    last_error TEXT,
    created_at TEXT NOT NULL,
    delivered_at TEXT
);

-- Due-poll index: the worker selects status='pending' AND next_attempt_at <= now.
CREATE INDEX idx_webhook_outbox_due ON webhook_outbox(status, next_attempt_at);
