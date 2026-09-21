CREATE TABLE stateful_runs (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL,
    goal TEXT NOT NULL,
    mode TEXT NOT NULL CHECK (mode IN ('autonomous', 'collaborative', 'socratic')),
    status TEXT NOT NULL CHECK (status IN (
        'pending', 'running', 'paused', 'completed', 'cancelled', 'blocked', 'failed'
    )),
    strategy TEXT,
    strategy_revision INTEGER NOT NULL CHECK (strategy_revision >= 0),
    result TEXT,
    revision INTEGER NOT NULL CHECK (revision > 0),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE INDEX stateful_runs_project
ON stateful_runs (project_id, updated_at_ms DESC, id);

CREATE TABLE stateful_run_threads (
    run_id TEXT NOT NULL REFERENCES stateful_runs(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    thread_id TEXT NOT NULL,
    PRIMARY KEY (run_id, thread_id),
    UNIQUE (run_id, position)
);

CREATE INDEX stateful_run_threads_thread
ON stateful_run_threads (thread_id, run_id);

CREATE TABLE stateful_obligations (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT UNIQUE NOT NULL,
    project_id TEXT NOT NULL,
    run_id TEXT NOT NULL REFERENCES stateful_runs(id) ON DELETE CASCADE,
    packet_json TEXT NOT NULL,
    provenance_source_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision = 1),
    created_at_ms INTEGER NOT NULL
);

CREATE INDEX stateful_obligations_run
ON stateful_obligations (project_id, run_id, sequence DESC);
