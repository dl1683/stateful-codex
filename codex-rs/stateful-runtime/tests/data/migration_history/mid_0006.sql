-- Host-observed receipts of a thread's work (edits, commands, plans, messages), appended as
-- each item completes, so a crash never loses a completed observation.
CREATE TABLE stateful_window_events (
    thread_id TEXT NOT NULL,
    seq INTEGER NOT NULL CHECK (seq > 0),
    event_key TEXT NOT NULL,
    project_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('user', 'command', 'edit', 'plan', 'message')),
    payload_json TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    PRIMARY KEY (thread_id, seq),
    UNIQUE (thread_id, event_key)
);

-- One publication of a thread's journal suffix (from_seq, through_seq] for one project (a thread
-- can be moved between projects, so each project's observations are published to it alone). The
-- content and entry identity are frozen when staged, so a retry writes exactly the same entry.
CREATE TABLE stateful_window_publications (
    thread_id TEXT NOT NULL,
    from_seq INTEGER NOT NULL CHECK (from_seq >= 0),
    through_seq INTEGER NOT NULL CHECK (through_seq > from_seq),
    project_id TEXT NOT NULL,
    entry_id TEXT NOT NULL,
    content TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'published')),
    created_at_ms INTEGER NOT NULL,
    published_at_ms INTEGER,
    PRIMARY KEY (thread_id, project_id, from_seq)
);

CREATE INDEX stateful_window_publications_pending
ON stateful_window_publications (project_id, state, created_at_ms);
