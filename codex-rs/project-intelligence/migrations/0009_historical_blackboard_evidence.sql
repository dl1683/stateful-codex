DROP TRIGGER blackboard_evidence_is_current_insert;

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
AND NOT EXISTS (
    SELECT 1
    FROM blackboard_entry_revisions AS revision
    JOIN blackboard_evidence_links AS previous
      ON previous.entry_id = revision.entry_id
     AND previous.revision = revision.revision - 1
    WHERE revision.entry_id = NEW.entry_id
      AND revision.revision = NEW.revision
      AND (
          revision.state IN ('superseded', 'tombstoned')
          OR revision.verification IN ('disputed', 'stale')
      )
      AND previous.context_map_entry_id = NEW.context_map_entry_id
      AND previous.source_fingerprint = NEW.source_fingerprint
      AND previous.first_line IS NEW.first_line
      AND previous.last_line IS NEW.last_line
)
BEGIN
    SELECT RAISE(ABORT, 'blackboard evidence is not current');
END;
