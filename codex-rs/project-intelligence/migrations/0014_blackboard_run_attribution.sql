ALTER TABLE blackboard_entry_revisions
ADD COLUMN agent_run_id TEXT
CHECK (agent_run_id IS NULL OR provenance_kind = 'agent');

ALTER TABLE blackboard_relations
ADD COLUMN agent_run_id TEXT
CHECK (agent_run_id IS NULL OR provenance_kind = 'agent');

CREATE INDEX blackboard_entry_revisions_agent_run
ON blackboard_entry_revisions (agent_run_id, entry_id);

CREATE INDEX blackboard_relations_agent_run
ON blackboard_relations (project_id, agent_run_id);

CREATE TABLE blackboard_attribution_metadata (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    attribution_enabled_at_ms INTEGER NOT NULL
);

INSERT INTO blackboard_attribution_metadata (id, attribution_enabled_at_ms)
VALUES (1, unixepoch('now') * 1000);
