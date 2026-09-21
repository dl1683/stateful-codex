CREATE TABLE hierarchy_nodes (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL,
    parent_id TEXT REFERENCES hierarchy_nodes(id) ON DELETE RESTRICT,
    kind TEXT NOT NULL CHECK (kind IN ('project', 'directory', 'file', 'region')),
    project_root TEXT,
    relative_path TEXT NOT NULL,
    anchor_scheme TEXT,
    anchor_locator TEXT,
    source_fingerprint TEXT,
    lifecycle TEXT NOT NULL CHECK (lifecycle IN ('active', 'missing', 'replaced')),
    revision INTEGER NOT NULL CHECK (revision > 0),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE UNIQUE INDEX hierarchy_nodes_location
ON hierarchy_nodes (
    project_id,
    kind,
    IFNULL(project_root, ''),
    relative_path,
    IFNULL(anchor_scheme, ''),
    IFNULL(anchor_locator, '')
);

CREATE INDEX hierarchy_nodes_children
ON hierarchy_nodes (project_id, parent_id, relative_path, kind);
