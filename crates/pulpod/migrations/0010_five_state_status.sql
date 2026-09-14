-- Five-state session model (ADR 0009): creating/active/idle/ready/stopped/lost
-- collapses to starting/working/waiting/done/lost. `ready` and `stopped` both
-- become `done` — both meant "not running, and resumable"; the *how it got
-- there* moves to the new `status_reason` column instead of being a
-- distinguishable top-level status. `lost` is unchanged.
--
-- `status_reason` qualifies `waiting` (`idle`, or `needs_input:<permission|
-- question|idle|other-label>`) and `done` (`exited`, `stopped`, or an
-- `InterventionCode` display string: `idle_timeout`, `budget_exceeded`,
-- `memory_pressure`). `starting`/`working`/`lost` never carry one.
ALTER TABLE sessions ADD COLUMN status_reason TEXT;

-- Reclassify while the old status names are still readable.
--
-- `ready` sessions had already exited cleanly (the fallback shell was the
-- only thing keeping them off `stopped`) — always "exited".
UPDATE sessions SET status_reason = 'exited' WHERE status = 'ready';

-- `stopped` sessions: an intervention-recorded code becomes the reason
-- verbatim when it's one this model recognizes; anything else (an explicit
-- `pulpo stop`, a clean shell exit with no intervention row, `user_stop`, or a
-- retired code like the removed burn-velocity governor's `burn_rate`) becomes
-- the generic `stopped`.
UPDATE sessions
SET status_reason = CASE
    WHEN intervention_code IN ('idle_timeout', 'budget_exceeded', 'memory_pressure')
        THEN intervention_code
    ELSE 'stopped'
END
WHERE status IN ('stopped', 'killed');

-- Migrate the `needs_input` metadata key (the harness-driven "blocked on me"
-- reason) into status_reason for sessions still waiting on it, then strip the
-- key from the metadata JSON blob — new code reads status_reason instead and
-- must never see a stale copy. json_extract/json_remove need SQLite's JSON1
-- functions, built into SQLite by default since 3.38 (this project bundles a
-- current SQLite via libsqlite3-sys).
UPDATE sessions
SET status_reason = 'needs_input:' || json_extract(metadata, '$.needs_input')
WHERE status = 'idle'
  AND metadata IS NOT NULL
  AND json_valid(metadata)
  AND json_extract(metadata, '$.needs_input') IS NOT NULL;

UPDATE sessions
SET metadata = json_remove(metadata, '$.needs_input')
WHERE metadata IS NOT NULL
  AND json_valid(metadata)
  AND json_extract(metadata, '$.needs_input') IS NOT NULL;

-- Every other `idle` session (no needs_input reason) is plain "idle".
UPDATE sessions SET status_reason = 'idle' WHERE status = 'idle' AND status_reason IS NULL;

-- Now rename the statuses themselves.
UPDATE sessions SET status = CASE status
    WHEN 'creating' THEN 'starting'
    WHEN 'active' THEN 'working'
    WHEN 'idle' THEN 'waiting'
    WHEN 'ready' THEN 'done'
    WHEN 'stopped' THEN 'done'
    WHEN 'killed' THEN 'done'
    ELSE status
END;

-- Rebuild the "one live session per name" unique index for the new status
-- set — `done`/`lost` sessions were never part of it (a name frees up once a
-- session finishes), and `ready` no longer exists as a distinct status.
DROP INDEX IF EXISTS idx_sessions_live_name;
CREATE UNIQUE INDEX idx_sessions_live_name
ON sessions(name) WHERE status IN ('starting', 'working', 'waiting');
