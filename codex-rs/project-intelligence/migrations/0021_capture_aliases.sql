CREATE TABLE capture_identity_aliases (
    project_id TEXT NOT NULL,
    entry_id TEXT NOT NULL REFERENCES blackboard_entries(id),
    normalizer_version TEXT NOT NULL,
    category TEXT NOT NULL,
    authority TEXT NOT NULL,
    scope_id TEXT NOT NULL,
    primary_words TEXT NOT NULL,
    retirement_words TEXT NOT NULL,
    retired INTEGER NOT NULL CHECK (retired IN (0,1)),
    PRIMARY KEY(entry_id, primary_words, category, authority, scope_id)
);
CREATE INDEX capture_alias_primary ON capture_identity_aliases(project_id, primary_words, category, authority, scope_id);
CREATE INDEX capture_alias_retirement ON capture_identity_aliases(project_id, retirement_words, retired, scope_id);
CREATE TABLE capture_identity_coverage (
    project_id TEXT PRIMARY KEY,
    after_rowid INTEGER NOT NULL DEFAULT 0,
    watermark INTEGER NOT NULL,
    blocked_reason TEXT
);
-- Pin the upgrade domain. All new revisions go through the common writer hooks.
INSERT INTO capture_identity_coverage(project_id, watermark)
SELECT entry.project_id, MAX(revision.rowid)
FROM blackboard_entries AS entry JOIN blackboard_entry_revisions AS revision ON revision.entry_id = entry.id
GROUP BY entry.project_id;
