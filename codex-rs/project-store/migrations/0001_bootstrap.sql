CREATE TABLE project_store_identity (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    project_uuid TEXT NOT NULL,
    store_uuid TEXT NOT NULL,
    format_version INTEGER NOT NULL,
    min_reader_version INTEGER NOT NULL,
    min_writer_version INTEGER NOT NULL
) STRICT;

CREATE TABLE project_store_migrations (
    domain TEXT NOT NULL,
    version INTEGER NOT NULL CHECK (version > 0),
    digest BLOB NOT NULL CHECK (length(digest) = 32),
    PRIMARY KEY (domain, version)
) STRICT;
