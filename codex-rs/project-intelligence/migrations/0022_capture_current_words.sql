-- Revision-bound emitted words are a rebuildable eligibility projection, not history.
CREATE TABLE capture_current_words (
    entry_id TEXT NOT NULL REFERENCES blackboard_entries(id),
    field TEXT NOT NULL,
    revision INTEGER NOT NULL,
    scope_id TEXT NOT NULL,
    primary_words TEXT NOT NULL,
    retirement_words TEXT NOT NULL,
    PRIMARY KEY(entry_id, field)
);
-- Existing stores must prove their current emitted fields through bounded maintenance.
INSERT INTO capture_identity_coverage(project_id, watermark)
SELECT entry.project_id, MAX(revision.rowid)
FROM blackboard_entries AS entry JOIN blackboard_entry_revisions AS revision ON revision.entry_id = entry.id
GROUP BY entry.project_id
ON CONFLICT(project_id) DO UPDATE SET after_rowid = 0, watermark = excluded.watermark;
