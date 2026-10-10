-- A run the host ended with its first answering task's final answer. One record per run,
-- written in the run's terminal transaction together with the Completed status. The record
-- keeps the answer exactly as the user received it and the basis that states what the host
-- observed; the answer is judged by the user, not verified by the host.
CREATE TABLE stateful_host_answers (
    run_id TEXT PRIMARY KEY NOT NULL REFERENCES stateful_runs(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    answer TEXT NOT NULL,
    basis TEXT NOT NULL,
    committed_at_ms INTEGER NOT NULL
);
