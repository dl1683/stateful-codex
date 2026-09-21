CREATE TABLE blackboard_relations (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL,
    from_entry_id TEXT NOT NULL REFERENCES blackboard_entries(id) ON DELETE RESTRICT,
    to_entry_id TEXT NOT NULL REFERENCES blackboard_entries(id) ON DELETE RESTRICT,
    kind TEXT NOT NULL CHECK (kind IN ('supports', 'contradicts', 'depends_on', 'related_to')),
    note TEXT,
    confidence_basis_points INTEGER NOT NULL
        CHECK (confidence_basis_points BETWEEN 0 AND 10000),
    provenance_kind TEXT NOT NULL CHECK (provenance_kind IN (
        'user', 'agent', 'maintenance', 'import'
    )),
    provenance_source_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision = 1),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    CHECK (from_entry_id <> to_entry_id)
);

CREATE INDEX blackboard_relations_from
ON blackboard_relations (project_id, from_entry_id, id);

CREATE INDEX blackboard_relations_to
ON blackboard_relations (project_id, to_entry_id, id);

CREATE TRIGGER blackboard_relation_endpoints_insert
BEFORE INSERT ON blackboard_relations
WHEN NOT EXISTS (
    SELECT 1 FROM blackboard_entries AS source
    JOIN blackboard_entries AS target ON target.id = NEW.to_entry_id
    WHERE source.id = NEW.from_entry_id
      AND source.project_id = NEW.project_id
      AND target.project_id = NEW.project_id
)
BEGIN
    SELECT RAISE(ABORT, 'blackboard relation endpoint is outside the project');
END;

CREATE TRIGGER project_revision_blackboard_relation_insert
AFTER INSERT ON blackboard_relations
BEGIN
    INSERT INTO project_intelligence_revisions (project_id, revision)
    VALUES (NEW.project_id, 1)
    ON CONFLICT(project_id) DO UPDATE SET revision = revision + 1;
END;
