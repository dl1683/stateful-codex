CREATE TABLE blackboard_premise_links (
    entry_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    position INTEGER NOT NULL CHECK (position >= 0),
    premise_entry_id TEXT NOT NULL,
    premise_revision INTEGER NOT NULL CHECK (premise_revision > 0),
    PRIMARY KEY (entry_id, revision, position),
    UNIQUE (entry_id, revision, premise_entry_id),
    FOREIGN KEY (entry_id, revision)
        REFERENCES blackboard_entry_revisions(entry_id, revision) ON DELETE CASCADE,
    FOREIGN KEY (premise_entry_id, premise_revision)
        REFERENCES blackboard_entry_revisions(entry_id, revision) ON DELETE RESTRICT,
    CHECK (entry_id <> premise_entry_id)
);

CREATE INDEX blackboard_premises_target
ON blackboard_premise_links (premise_entry_id, premise_revision, entry_id, revision);

CREATE TRIGGER blackboard_premise_is_current_insert
BEFORE INSERT ON blackboard_premise_links
WHEN NOT EXISTS (
    SELECT 1
    FROM blackboard_entries AS owner
    JOIN blackboard_entry_revisions AS owner_revision
      ON owner_revision.entry_id = owner.id
     AND owner_revision.revision = owner.revision
    JOIN blackboard_entries AS premise
      ON premise.id = NEW.premise_entry_id
     AND premise.project_id = owner.project_id
    JOIN blackboard_entry_revisions AS premise_revision
      ON premise_revision.entry_id = premise.id
     AND premise_revision.revision = premise.revision
    WHERE owner.id = NEW.entry_id
      AND owner.revision = NEW.revision
      AND premise.revision = NEW.premise_revision
      AND premise_revision.state = 'active'
      AND premise_revision.verification IN ('source_verified', 'user_confirmed')
)
AND NOT EXISTS (
    SELECT 1
    FROM blackboard_entry_revisions AS owner_revision
    JOIN blackboard_premise_links AS previous
      ON previous.entry_id = owner_revision.entry_id
     AND previous.revision = owner_revision.revision - 1
    WHERE owner_revision.entry_id = NEW.entry_id
      AND owner_revision.revision = NEW.revision
      AND (
          owner_revision.state IN ('superseded', 'tombstoned')
          OR owner_revision.verification IN ('disputed', 'stale')
      )
      AND previous.premise_entry_id = NEW.premise_entry_id
      AND previous.premise_revision = NEW.premise_revision
)
BEGIN
    SELECT RAISE(ABORT, 'blackboard premise is not current and trusted');
END;
