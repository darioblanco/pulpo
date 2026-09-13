-- Web Push and the durable webhook outbox were removed: notifications now
-- flow through a single channel, `[[webhooks]]`, delivered as a plain POST
-- with a fixed retry schedule from an in-memory queue (see
-- notifications::webhook). Drop their tables; there is no separate
-- action-token table to drop — action tokens were stateless HMAC-signed
-- capabilities, never persisted.
DROP TABLE IF EXISTS push_subscriptions;
DROP TABLE IF EXISTS webhook_outbox;
