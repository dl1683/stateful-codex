-- How Stateful renders each context window of a thread, decided once when the window opens.
CREATE TABLE stateful_context_windows (
    thread_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    window_id TEXT NOT NULL,
    window_number INTEGER NOT NULL CHECK (window_number >= 0),
    mode TEXT NOT NULL CHECK (mode IN ('full', 'continuation')),
    reason TEXT NOT NULL CHECK (reason IN ('threadStart', 'compaction', 'reset', 'unknown')),
    created_at_ms INTEGER NOT NULL,
    PRIMARY KEY (thread_id, project_id, window_id)
);
