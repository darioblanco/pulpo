-- Harness adapters: normalize agent lifecycle events (SessionStart, Stop, Notification,
-- ...) instead of scraping tmux scrollback. `harness` is the adapter id ("claude"),
-- `harness_session_id` is the harness's own session/thread id (used to resume the same
-- conversation), and `harness_last_event_at` tells the watchdog events are flowing for
-- this session, so it stops applying scrollback heuristics to it.
ALTER TABLE sessions ADD COLUMN harness TEXT;
ALTER TABLE sessions ADD COLUMN harness_session_id TEXT;
ALTER TABLE sessions ADD COLUMN harness_last_event_at TEXT;
