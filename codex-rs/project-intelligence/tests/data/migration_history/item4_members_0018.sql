-- The ordered members of each capture group and where the group was read from. A member is
-- an entry the capture saved or found already saved, or a unit it recognized but could not
-- keep; an entry first saved by an earlier group keeps that group in its context and is
-- listed here too, so every group can be shown whole.

ALTER TABLE capture_groups ADD COLUMN source_locator TEXT NULL;
ALTER TABLE capture_groups ADD COLUMN source_digest TEXT NULL;

CREATE TABLE capture_group_members (
    project_id TEXT NOT NULL,
    group_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    entry_id TEXT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN (
        'saved', 'already_present', 'not_restored', 'omitted'
    )),
    note TEXT NULL,
    PRIMARY KEY (project_id, group_id, ordinal),
    CHECK ((outcome IN ('saved', 'already_present')) = (entry_id IS NOT NULL))
);

CREATE INDEX capture_group_members_entry
ON capture_group_members (project_id, entry_id);
