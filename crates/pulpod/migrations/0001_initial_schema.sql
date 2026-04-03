CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    workdir TEXT NOT NULL,
    provider TEXT NOT NULL,
    prompt TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'creating',
    mode TEXT NOT NULL DEFAULT 'interactive',
    conversation_id TEXT,
    exit_code INTEGER,
    backend_session_id TEXT,
    output_snapshot TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    intervention_reason TEXT,
    intervention_at TEXT,
    last_output_at TEXT,
    idle_since TEXT,
    intervention_code TEXT,
    metadata TEXT,
    ink TEXT,
    command TEXT DEFAULT '',
    description TEXT,
    idle_threshold_secs INTEGER,
    worktree_path TEXT,
    worktree_branch TEXT,
    sandbox INTEGER DEFAULT 0,
    runtime TEXT NOT NULL DEFAULT 'tmux',
    git_branch TEXT,
    git_commit TEXT,
    git_files_changed INTEGER,
    git_insertions INTEGER,
    git_deletions INTEGER,
    git_ahead INTEGER
);

CREATE TABLE intervention_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    reason TEXT NOT NULL,
    created_at TEXT NOT NULL,
    code TEXT
);

CREATE UNIQUE INDEX idx_sessions_live_name
ON sessions(name) WHERE status IN ('creating', 'active', 'idle', 'ready');

CREATE TABLE push_subscriptions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    endpoint TEXT NOT NULL UNIQUE,
    p256dh TEXT NOT NULL,
    auth TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE secrets (
    name TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    created_at TEXT NOT NULL,
    env TEXT
);

CREATE TABLE schedules (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    cron TEXT NOT NULL,
    command TEXT NOT NULL DEFAULT '',
    workdir TEXT NOT NULL,
    target_node TEXT,
    ink TEXT,
    description TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    last_run_at TEXT,
    last_session_id TEXT,
    created_at TEXT NOT NULL,
    runtime TEXT,
    secrets TEXT NOT NULL DEFAULT '[]',
    worktree INTEGER,
    worktree_base TEXT
);

CREATE TABLE controller_session_index (
    session_id TEXT PRIMARY KEY,
    node_name TEXT NOT NULL,
    node_address TEXT,
    session_name TEXT NOT NULL,
    status TEXT NOT NULL,
    command TEXT,
    updated_at TEXT NOT NULL
);

CREATE TABLE controller_nodes (
    node_name TEXT PRIMARY KEY,
    last_seen_at TEXT NOT NULL
);

CREATE TABLE controller_enrolled_nodes (
    node_name TEXT PRIMARY KEY,
    token_hash TEXT NOT NULL UNIQUE,
    last_seen_at TEXT,
    last_seen_address TEXT
);
