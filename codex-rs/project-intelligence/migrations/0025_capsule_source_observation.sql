-- A newly observed part can make its turn ineligible when its digest is already excluded, so
-- every source observation conservatively advances the capsule eligibility generation.
CREATE TRIGGER capsule_generation_source_insert AFTER INSERT ON capture_sources
BEGIN INSERT INTO capsule_eligibility_generations VALUES (NEW.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
