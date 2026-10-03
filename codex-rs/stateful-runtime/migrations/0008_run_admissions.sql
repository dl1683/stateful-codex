-- The run a thread's turns are bound to. The generation advances whenever the bound run
-- changes, so every admitted turn names exactly which binding it ran under.
CREATE TABLE stateful_thread_run_bindings (
    thread_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES stateful_runs(id) ON DELETE CASCADE,
    generation INTEGER NOT NULL CHECK (generation > 0),
    updated_at_ms INTEGER NOT NULL
);

-- One immutable admission per turn, recorded before the turn samples.
CREATE TABLE stateful_run_turn_admissions (
    turn_id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL,
    run_id TEXT NOT NULL REFERENCES stateful_runs(id) ON DELETE CASCADE,
    generation INTEGER NOT NULL CHECK (generation > 0),
    created_at_ms INTEGER NOT NULL
);

CREATE INDEX stateful_run_turn_admissions_thread
ON stateful_run_turn_admissions (thread_id, created_at_ms);
