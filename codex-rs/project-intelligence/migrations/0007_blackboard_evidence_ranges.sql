ALTER TABLE blackboard_evidence_links
ADD COLUMN first_line INTEGER CHECK (first_line IS NULL OR first_line > 0);

ALTER TABLE blackboard_evidence_links
ADD COLUMN last_line INTEGER CHECK (last_line IS NULL OR last_line > 0);

CREATE TRIGGER blackboard_evidence_range_is_valid_insert
BEFORE INSERT ON blackboard_evidence_links
WHEN (NEW.first_line IS NULL) <> (NEW.last_line IS NULL)
  OR NEW.first_line > NEW.last_line
  OR NEW.last_line - NEW.first_line >= 2000
BEGIN
    SELECT RAISE(ABORT, 'blackboard evidence line range is invalid');
END;

CREATE TRIGGER blackboard_evidence_range_is_valid_update
BEFORE UPDATE OF first_line, last_line ON blackboard_evidence_links
WHEN (NEW.first_line IS NULL) <> (NEW.last_line IS NULL)
  OR NEW.first_line > NEW.last_line
  OR NEW.last_line - NEW.first_line >= 2000
BEGIN
    SELECT RAISE(ABORT, 'blackboard evidence line range is invalid');
END;
