CREATE TABLE project_index_refresh_status (
    project_id TEXT PRIMARY KEY NOT NULL,
    inventory_complete INTEGER NOT NULL CHECK (inventory_complete IN (0, 1)),
    region_coverage_complete INTEGER NOT NULL CHECK (region_coverage_complete IN (0, 1)),
    files_indexed INTEGER NOT NULL CHECK (files_indexed >= 0),
    regions_indexed INTEGER NOT NULL CHECK (regions_indexed >= 0),
    files_skipped INTEGER NOT NULL CHECK (files_skipped >= 0),
    missing_files INTEGER NOT NULL CHECK (missing_files >= 0),
    truncated INTEGER NOT NULL CHECK (truncated IN (0, 1)),
    scan_duration_ms INTEGER NOT NULL CHECK (scan_duration_ms >= 0),
    publication_duration_ms INTEGER NOT NULL CHECK (publication_duration_ms >= 0),
    completed_at_ms INTEGER NOT NULL
);
