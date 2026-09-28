CREATE TABLE region_extraction_attestations (
    region_node_id TEXT PRIMARY KEY NOT NULL
        REFERENCES hierarchy_nodes(id) ON DELETE CASCADE,
    extractor_name TEXT NOT NULL,
    extractor_version TEXT NOT NULL,
    canonical_representation_digest TEXT NOT NULL
);
