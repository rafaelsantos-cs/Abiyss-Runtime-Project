PRAGMA journal_mode=WAL;
PRAGMA synchronous=FULL;
PRAGMA foreign_keys=ON;

CREATE TABLE IF NOT EXISTS runtime_queries (
    query_id TEXT PRIMARY KEY,
    query_type TEXT NOT NULL CHECK(query_type IN ('Aquery','Squery')),
    priority INTEGER NOT NULL,
    state TEXT NOT NULL,
    created_at REAL NOT NULL,
    updated_at REAL NOT NULL,
    attempt INTEGER NOT NULL DEFAULT 0,
    checkpoint_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS query_steps (
    query_id TEXT NOT NULL REFERENCES runtime_queries(query_id) ON DELETE CASCADE,
    step_index INTEGER NOT NULL,
    tool_name TEXT NOT NULL,
    arguments_json TEXT NOT NULL,
    state TEXT NOT NULL,
    result_json TEXT,
    started_at REAL,
    completed_at REAL,
    PRIMARY KEY(query_id, step_index)
);

CREATE TABLE IF NOT EXISTS tool_calls (
    interaction_id TEXT NOT NULL,
    call_id TEXT NOT NULL,
    query_id TEXT NOT NULL REFERENCES runtime_queries(query_id),
    created_at REAL NOT NULL,
    UNIQUE(interaction_id, call_id)
);

CREATE TABLE IF NOT EXISTS runtime_events (
    id INTEGER PRIMARY KEY,
    event_type TEXT NOT NULL,
    query_id TEXT,
    payload_json TEXT NOT NULL,
    created_at REAL NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_runtime_queries_state_priority
    ON runtime_queries(state, priority DESC, created_at ASC);
CREATE INDEX IF NOT EXISTS idx_runtime_events_query
    ON runtime_events(query_id, id DESC);
