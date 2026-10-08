ALTER TABLE threads ADD COLUMN project_binding_generation INTEGER NOT NULL DEFAULT 1;
CREATE TRIGGER threads_project_binding_generation
AFTER UPDATE OF project_id ON threads
WHEN OLD.project_id IS NOT NEW.project_id
BEGIN
    UPDATE threads SET project_binding_generation = OLD.project_binding_generation + 1 WHERE id = NEW.id;
END;
