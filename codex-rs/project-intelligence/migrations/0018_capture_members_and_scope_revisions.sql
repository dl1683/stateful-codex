-- What each unit of a capture became, in the order the user wrote them.
CREATE TABLE capture_group_members (
    project_id TEXT NOT NULL,
    group_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    entry_id TEXT NULL,
    revision INTEGER NULL,
    outcome TEXT NOT NULL CHECK (outcome IN (
        'saved', 'already_present', 'pending', 'omitted', 'failed'
    )),
    preview TEXT NOT NULL,
    reason TEXT NULL,
    PRIMARY KEY (project_id, group_id, ordinal),
    FOREIGN KEY (project_id, group_id)
        REFERENCES capture_groups(project_id, group_id) ON DELETE CASCADE
);

-- Which rules apply where changes when a scope opens or ends or a thread joins one, so
-- positional root aliases from before such a change are never reused.
CREATE TRIGGER project_revision_scope_insert
AFTER INSERT ON knowledge_scopes
BEGIN
    INSERT INTO project_intelligence_revisions (project_id, revision)
    VALUES (NEW.project_id, 1)
    ON CONFLICT(project_id) DO UPDATE SET revision = revision + 1;
END;

CREATE TRIGGER project_revision_scope_update
AFTER UPDATE OF state ON knowledge_scopes
WHEN OLD.state <> NEW.state
BEGIN
    INSERT INTO project_intelligence_revisions (project_id, revision)
    VALUES (NEW.project_id, 1)
    ON CONFLICT(project_id) DO UPDATE SET revision = revision + 1;
END;

CREATE TRIGGER project_revision_scope_binding_insert
AFTER INSERT ON knowledge_scope_bindings
BEGIN
    INSERT INTO project_intelligence_revisions (project_id, revision)
    VALUES (NEW.project_id, 1)
    ON CONFLICT(project_id) DO UPDATE SET revision = revision + 1;
END;

CREATE TRIGGER project_revision_scope_binding_update
AFTER UPDATE OF scope_id ON knowledge_scope_bindings
WHEN OLD.scope_id <> NEW.scope_id
BEGIN
    INSERT INTO project_intelligence_revisions (project_id, revision)
    VALUES (NEW.project_id, 1)
    ON CONFLICT(project_id) DO UPDATE SET revision = revision + 1;
END;

CREATE TRIGGER project_revision_scope_binding_delete
AFTER DELETE ON knowledge_scope_bindings
BEGIN
    INSERT INTO project_intelligence_revisions (project_id, revision)
    VALUES (OLD.project_id, 1)
    ON CONFLICT(project_id) DO UPDATE SET revision = revision + 1;
END;
