CREATE TABLE blackboard_evidence_links_v2 (
    entry_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    position INTEGER NOT NULL CHECK (position >= 0),
    context_map_entry_id TEXT NOT NULL
        REFERENCES context_map_entries(id) ON DELETE RESTRICT,
    source_fingerprint TEXT NOT NULL,
    first_line INTEGER CHECK (first_line IS NULL OR first_line > 0),
    last_line INTEGER CHECK (last_line IS NULL OR last_line > 0),
    PRIMARY KEY (entry_id, revision, position),
    FOREIGN KEY (entry_id, revision)
        REFERENCES blackboard_entry_revisions(entry_id, revision) ON DELETE CASCADE,
    CHECK ((first_line IS NULL) = (last_line IS NULL)),
    CHECK (first_line IS NULL OR first_line <= last_line),
    CHECK (first_line IS NULL OR last_line - first_line < 2000)
);

INSERT INTO blackboard_evidence_links_v2 (
    entry_id, revision, position, context_map_entry_id, source_fingerprint,
    first_line, last_line
)
SELECT entry_id, revision, position, context_map_entry_id, source_fingerprint,
       first_line, last_line
FROM blackboard_evidence_links;

DROP TABLE blackboard_evidence_links;

ALTER TABLE blackboard_evidence_links_v2
RENAME TO blackboard_evidence_links;

CREATE UNIQUE INDEX blackboard_evidence_locator_unique
ON blackboard_evidence_links (
    entry_id,
    revision,
    context_map_entry_id,
    COALESCE(first_line, 0),
    COALESCE(last_line, 0)
);

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
