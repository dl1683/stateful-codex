CREATE TABLE blackboard_entries (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL,
    node_id TEXT NOT NULL REFERENCES hierarchy_nodes(id) ON DELETE RESTRICT,
    revision INTEGER NOT NULL CHECK (revision > 0),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE INDEX blackboard_entries_node
ON blackboard_entries (project_id, node_id, id);

CREATE TABLE blackboard_entry_revisions (
    entry_id TEXT NOT NULL REFERENCES blackboard_entries(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision > 0),
    kind TEXT NOT NULL CHECK (kind IN (
        'instruction', 'fact', 'claim', 'number', 'decision', 'strategy',
        'question', 'contradiction', 'failure', 'rejected_approach', 'signal', 'note'
    )),
    content TEXT NOT NULL,
    structured_value TEXT,
    structured_unit TEXT,
    confidence_basis_points INTEGER NOT NULL
        CHECK (confidence_basis_points BETWEEN 0 AND 10000),
    verification TEXT NOT NULL CHECK (verification IN (
        'unverified', 'source_verified', 'user_confirmed', 'disputed', 'stale'
    )),
    importance TEXT NOT NULL CHECK (importance IN ('critical', 'high', 'normal', 'low')),
    root_promotion TEXT NOT NULL CHECK (root_promotion IN (
        'not_promoted', 'candidate', 'promoted'
    )),
    state TEXT NOT NULL CHECK (state IN ('active', 'superseded', 'tombstoned')),
    superseded_by TEXT REFERENCES blackboard_entries(id) ON DELETE RESTRICT,
    provenance_kind TEXT NOT NULL CHECK (provenance_kind IN (
        'user', 'agent', 'maintenance', 'import'
    )),
    provenance_source_id TEXT NOT NULL,
    recorded_at_ms INTEGER NOT NULL,
    PRIMARY KEY (entry_id, revision),
    CHECK (structured_value IS NOT NULL OR structured_unit IS NULL),
    CHECK (
        (state = 'superseded' AND superseded_by IS NOT NULL)
        OR (state IN ('active', 'tombstoned') AND superseded_by IS NULL)
    ),
    CHECK (superseded_by IS NULL OR superseded_by <> entry_id)
);

CREATE TABLE blackboard_evidence_links (
    entry_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    position INTEGER NOT NULL CHECK (position >= 0),
    context_map_entry_id TEXT NOT NULL
        REFERENCES context_map_entries(id) ON DELETE RESTRICT,
    source_fingerprint TEXT NOT NULL,
    PRIMARY KEY (entry_id, revision, position),
    UNIQUE (entry_id, revision, context_map_entry_id),
    FOREIGN KEY (entry_id, revision)
        REFERENCES blackboard_entry_revisions(entry_id, revision) ON DELETE CASCADE
);

CREATE TRIGGER blackboard_revision_is_current_insert
BEFORE INSERT ON blackboard_entry_revisions
WHEN NOT EXISTS (
    SELECT 1
    FROM blackboard_entries
    WHERE id = NEW.entry_id
      AND revision = NEW.revision
)
BEGIN
    SELECT RAISE(ABORT, 'blackboard revision is not current');
END;

CREATE TRIGGER blackboard_evidence_is_current_insert
BEFORE INSERT ON blackboard_evidence_links
WHEN NOT EXISTS (
    SELECT 1
    FROM blackboard_entry_revisions AS revision
    JOIN blackboard_entries AS entry ON entry.id = revision.entry_id
    JOIN context_map_entries AS evidence ON evidence.id = NEW.context_map_entry_id
    JOIN hierarchy_nodes AS source ON source.id = evidence.node_id
    WHERE revision.entry_id = NEW.entry_id
      AND revision.revision = NEW.revision
      AND entry.project_id = evidence.project_id
      AND evidence.source_fingerprint = NEW.source_fingerprint
      AND source.project_id = evidence.project_id
      AND source.lifecycle = 'active'
      AND source.source_fingerprint = evidence.source_fingerprint
)
BEGIN
    SELECT RAISE(ABORT, 'blackboard evidence is not current');
END;
