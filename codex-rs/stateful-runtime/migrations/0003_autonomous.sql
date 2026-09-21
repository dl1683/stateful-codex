ALTER TABLE stateful_runs
ADD COLUMN max_continuations INTEGER NOT NULL DEFAULT 12 CHECK (max_continuations > 0);

ALTER TABLE stateful_runs
ADD COLUMN max_elapsed_seconds INTEGER NOT NULL DEFAULT 3600 CHECK (max_elapsed_seconds >= 60);

ALTER TABLE stateful_runs
ADD COLUMN continuations_used INTEGER NOT NULL DEFAULT 0 CHECK (continuations_used >= 0);

CREATE TABLE stateful_run_leases (
    run_id TEXT PRIMARY KEY NOT NULL REFERENCES stateful_runs(id) ON DELETE CASCADE,
    owner_id TEXT NOT NULL,
    lease_expires_at_ms INTEGER NOT NULL,
    previous_turn_id TEXT NOT NULL
);

CREATE TABLE stateful_run_continuations (
    run_id TEXT NOT NULL REFERENCES stateful_runs(id) ON DELETE CASCADE,
    previous_turn_id TEXT NOT NULL,
    claimed_at_ms INTEGER NOT NULL,
    PRIMARY KEY (run_id, previous_turn_id)
);
