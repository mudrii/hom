-- HOM initial schema

-- Workflow executions
CREATE TABLE IF NOT EXISTS workflows (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    definition_path TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending',
    variables       TEXT,  -- JSON
    started_at      INTEGER,
    completed_at    INTEGER,
    error           TEXT
);

-- Individual step results
CREATE TABLE IF NOT EXISTS steps (
    id              TEXT PRIMARY KEY,
    workflow_id     TEXT NOT NULL REFERENCES workflows(id),
    step_name       TEXT NOT NULL,
    harness         TEXT NOT NULL,
    model           TEXT,
    status          TEXT NOT NULL DEFAULT 'pending',
    prompt          TEXT,
    output          TEXT,
    error           TEXT,
    pane_id         INTEGER,
    tokens_input    INTEGER DEFAULT 0,
    tokens_output   INTEGER DEFAULT 0,
    cost_usd        REAL DEFAULT 0.0,
    started_at      INTEGER,
    completed_at    INTEGER,
    duration_ms     INTEGER,
    attempt         INTEGER DEFAULT 1
);

-- Session persistence
CREATE TABLE IF NOT EXISTS sessions (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    layout          TEXT NOT NULL,  -- JSON serialized Layout
    panes           TEXT NOT NULL,  -- JSON array of pane configs
    created_at      INTEGER,
    updated_at      INTEGER
);

-- Cost tracking
CREATE TABLE IF NOT EXISTS cost_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    pane_id         INTEGER,
    harness         TEXT NOT NULL,
    model           TEXT,
    tokens_input    INTEGER,
    tokens_output   INTEGER,
    cost_usd        REAL,
    timestamp       INTEGER
);

-- Workflow checkpoints (crash recovery)
CREATE TABLE IF NOT EXISTS checkpoints (
    workflow_id     TEXT NOT NULL,
    checkpoint_json TEXT NOT NULL,
    created_at      INTEGER NOT NULL,
    PRIMARY KEY (workflow_id)
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_steps_workflow ON steps(workflow_id);
CREATE INDEX IF NOT EXISTS idx_cost_log_harness ON cost_log(harness);
CREATE INDEX IF NOT EXISTS idx_cost_log_timestamp ON cost_log(timestamp);
