CREATE TABLE stateful_turn_measurements (
    run_id TEXT NOT NULL REFERENCES stateful_runs(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('completed', 'failed', 'aborted')),
    duration_ms INTEGER NOT NULL CHECK (duration_ms >= 0),
    attribution_counters_json TEXT NOT NULL,
    trajectory_json TEXT,
    token_usage_json TEXT,
    completed_at_ms INTEGER CHECK (completed_at_ms IS NULL OR completed_at_ms >= 0),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY (run_id, turn_id)
);

CREATE UNIQUE INDEX stateful_turn_measurements_thread_turn
ON stateful_turn_measurements (thread_id, turn_id);

CREATE INDEX stateful_turn_measurements_project
ON stateful_turn_measurements (
    project_id,
    created_at_ms DESC,
    run_id DESC,
    turn_id DESC
);
