CREATE TABLE context_map_entries (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL,
    node_id TEXT NOT NULL REFERENCES hierarchy_nodes(id) ON DELETE RESTRICT,
    source_fingerprint TEXT NOT NULL,
    description TEXT NOT NULL,
    coverage TEXT NOT NULL CHECK (coverage IN ('complete', 'partial')),
    revision INTEGER NOT NULL CHECK (revision > 0),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    last_verified_at_ms INTEGER,
    UNIQUE (project_id, node_id)
);

CREATE TABLE context_map_routing_terms (
    entry_id TEXT NOT NULL REFERENCES context_map_entries(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    term TEXT NOT NULL,
    PRIMARY KEY (entry_id, position),
    UNIQUE (entry_id, term)
);

CREATE INDEX context_map_routing_terms_term
ON context_map_routing_terms (term, entry_id);

CREATE TRIGGER context_map_current_source_insert
BEFORE INSERT ON context_map_entries
WHEN NOT EXISTS (
    SELECT 1
    FROM hierarchy_nodes
    WHERE id = NEW.node_id
      AND project_id = NEW.project_id
      AND kind IN ('file', 'region')
      AND lifecycle = 'active'
      AND source_fingerprint = NEW.source_fingerprint
)
BEGIN
    SELECT RAISE(ABORT, 'context-map source is not current');
END;
