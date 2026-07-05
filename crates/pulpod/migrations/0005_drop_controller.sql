-- Controller/node relay mode was removed: every pulpod is standalone and reached
-- directly (peer registry + Tailscale). Drop the controller-only tables and the
-- schedules.target_node column (schedules now always fire on the local node).
DROP TABLE IF EXISTS controller_session_index;
DROP TABLE IF EXISTS controller_nodes;
DROP TABLE IF EXISTS controller_enrolled_nodes;
ALTER TABLE schedules DROP COLUMN target_node;
