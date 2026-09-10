-- The secrets store was removed: every supported agent (Claude Code, Codex, pi, Gemini)
-- reads its own credentials from its own config, and env vars for a session can be
-- exported in the shell or wrapped into the command instead. Drop the secrets table
-- and the schedules.secrets column (bundled libsqlite3-sys 0.30.1 vendors SQLite
-- 3.46.0, well past the 3.35 minimum for ALTER TABLE DROP COLUMN).
DROP TABLE IF EXISTS secrets;
ALTER TABLE schedules DROP COLUMN secrets;
