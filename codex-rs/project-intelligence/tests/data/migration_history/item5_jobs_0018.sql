-- Progress of qualifying remembered assertions against committed source changes, one job per
-- project root. The qualified head is the last commit every eligible assertion was checked
-- against; it is separate from the last observed checkout. The manifest is the bounded list of
-- paths that changed between the qualified head and the target (opaque to storage). Entries
-- are scanned in insertion order up to the watermark; the cursor is the last examined entry.

CREATE TABLE qualification_jobs (
    project_id TEXT NOT NULL,
    project_root TEXT NOT NULL,
    qualified_head TEXT NOT NULL,
    target_head TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK (generation > 0),
    state TEXT NOT NULL CHECK (state IN ('scanning', 'complete', 'blocked')),
    manifest TEXT NULL,
    reason TEXT NULL,
    watermark INTEGER NOT NULL CHECK (watermark >= 0),
    cursor INTEGER NOT NULL CHECK (cursor >= 0),
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY (project_id, project_root)
);
