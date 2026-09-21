CREATE VIRTUAL TABLE blackboard_search USING fts5(
    entry_id UNINDEXED,
    content,
    structured_value,
    tokenize = 'unicode61'
);

INSERT INTO blackboard_search (entry_id, content, structured_value)
SELECT entry.id, revision.content, COALESCE(revision.structured_value, '')
FROM blackboard_entries AS entry
JOIN blackboard_entry_revisions AS revision
  ON revision.entry_id = entry.id AND revision.revision = entry.revision;

CREATE TABLE project_intelligence_revisions (
    project_id TEXT PRIMARY KEY NOT NULL,
    revision INTEGER NOT NULL CHECK (revision >= 0)
);

INSERT INTO project_intelligence_revisions (project_id, revision)
SELECT project_id, SUM(change_count)
FROM (
    SELECT project_id, COUNT(*) AS change_count FROM hierarchy_nodes GROUP BY project_id
    UNION ALL
    SELECT project_id, SUM(revision) AS change_count FROM context_map_entries GROUP BY project_id
    UNION ALL
    SELECT entry.project_id, COUNT(*) AS change_count
    FROM blackboard_entry_revisions AS revision
    JOIN blackboard_entries AS entry ON entry.id = revision.entry_id
    GROUP BY entry.project_id
)
GROUP BY project_id;

CREATE TRIGGER blackboard_search_current_revision_insert
AFTER INSERT ON blackboard_entry_revisions
WHEN EXISTS (
    SELECT 1 FROM blackboard_entries
    WHERE id = NEW.entry_id AND revision = NEW.revision
)
BEGIN
    DELETE FROM blackboard_search WHERE entry_id = NEW.entry_id;
    INSERT INTO blackboard_search (entry_id, content, structured_value)
    VALUES (NEW.entry_id, NEW.content, COALESCE(NEW.structured_value, ''));
END;

CREATE TRIGGER project_revision_blackboard_insert
AFTER INSERT ON blackboard_entry_revisions
WHEN EXISTS (
    SELECT 1 FROM blackboard_entries
    WHERE id = NEW.entry_id AND revision = NEW.revision
)
BEGIN
    INSERT INTO project_intelligence_revisions (project_id, revision)
    SELECT project_id, 1 FROM blackboard_entries WHERE id = NEW.entry_id
    ON CONFLICT(project_id) DO UPDATE SET revision = revision + 1;
END;

CREATE TRIGGER project_revision_hierarchy_insert
AFTER INSERT ON hierarchy_nodes
BEGIN
    INSERT INTO project_intelligence_revisions (project_id, revision)
    VALUES (NEW.project_id, 1)
    ON CONFLICT(project_id) DO UPDATE SET revision = revision + 1;
END;

CREATE TRIGGER project_revision_hierarchy_update
AFTER UPDATE OF revision ON hierarchy_nodes
BEGIN
    INSERT INTO project_intelligence_revisions (project_id, revision)
    VALUES (NEW.project_id, 1)
    ON CONFLICT(project_id) DO UPDATE SET revision = revision + 1;
END;

CREATE TRIGGER project_revision_context_map_insert
AFTER INSERT ON context_map_entries
BEGIN
    INSERT INTO project_intelligence_revisions (project_id, revision)
    VALUES (NEW.project_id, 1)
    ON CONFLICT(project_id) DO UPDATE SET revision = revision + 1;
END;

CREATE TRIGGER project_revision_context_map_update
AFTER UPDATE OF revision ON context_map_entries
BEGIN
    INSERT INTO project_intelligence_revisions (project_id, revision)
    VALUES (NEW.project_id, 1)
    ON CONFLICT(project_id) DO UPDATE SET revision = revision + 1;
END;
