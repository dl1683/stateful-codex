CREATE TABLE stateful_steering (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL,
    run_id TEXT NOT NULL REFERENCES stateful_runs(id) ON DELETE CASCADE,
    input TEXT NOT NULL,
    affected_obligation_ids_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('submitted', 'acknowledged', 'applied', 'rejected')),
    resulting_strategy_revision INTEGER CHECK (resulting_strategy_revision >= 0),
    reason TEXT,
    revision INTEGER NOT NULL CHECK (revision > 0),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    CHECK (status <> 'applied' OR resulting_strategy_revision IS NOT NULL),
    CHECK (status <> 'rejected' OR reason IS NOT NULL)
);

CREATE INDEX stateful_steering_run
ON stateful_steering (project_id, run_id, created_at_ms, id);
