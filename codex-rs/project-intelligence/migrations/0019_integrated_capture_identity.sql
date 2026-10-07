-- Reconcile member outcomes without changing the applied continuity 0018.
ALTER TABLE capture_group_members RENAME TO continuity_capture_group_members;
CREATE TABLE capture_group_members (
    project_id TEXT NOT NULL,
    group_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    entry_id TEXT NULL,
    revision INTEGER NULL,
    outcome TEXT NOT NULL CHECK (outcome IN (
        'saved', 'already_present', 'pending', 'omitted', 'failed', 'not_restored'
    )),
    preview TEXT NOT NULL,
    reason TEXT NULL,
    PRIMARY KEY (project_id, group_id, ordinal),
    FOREIGN KEY (project_id, group_id)
        REFERENCES capture_groups(project_id, group_id) ON DELETE CASCADE
);
INSERT INTO capture_group_members SELECT * FROM continuity_capture_group_members;
DROP TABLE continuity_capture_group_members;

-- No-op direct additions bind their request and exact outcome with the journal transaction.
CREATE TABLE capture_action_outcomes (
    project_id TEXT NOT NULL,
    action_id TEXT NOT NULL,
    request_fingerprint TEXT NOT NULL,
    entry_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('saved', 'already_present', 'pending')),
    PRIMARY KEY (project_id, action_id)
);
