CREATE TABLE project_index_refresh_generation (
    project_id TEXT PRIMARY KEY NOT NULL,
    latest_generation INTEGER NOT NULL CHECK (latest_generation > 0)
);
