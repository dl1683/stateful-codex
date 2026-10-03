-- The task capsule a continuation window carries, captured once when the window opens.
CREATE TABLE stateful_task_capsules (
    thread_id TEXT NOT NULL,
    window_id TEXT NOT NULL,
    through_seq INTEGER NOT NULL CHECK (through_seq >= 0),
    body TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    PRIMARY KEY (thread_id, window_id)
);
