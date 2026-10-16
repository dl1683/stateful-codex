use codex_project_store::StoreIdentity;
use sqlx::SqliteConnection;

use super::ProjectKnowledgeReadError;
use super::ProjectKnowledgeSnapshot;

// All logical intelligence tables with project_id, including the unused FTS
// projection. SQLite-owned FTS shadows/global feature metadata carry no owner.
pub(super) const PROJECT_TABLES: &[&str] = &[
    "blackboard_entries", "blackboard_relations", "capsule_eligibility_generations",
    "capture_action_outcomes", "capture_group_actions", "capture_group_members",
    "capture_groups", "capture_identity_aliases", "capture_identity_coverage",
    "capture_projection_coverage", "capture_source_exclusions", "capture_source_omissions",
    "capture_source_terms", "capture_sources", "context_map_entries", "context_map_search",
    "hierarchy_nodes", "knowledge_context", "knowledge_scope_bindings", "knowledge_scopes",
    "knowledge_source_sequences", "memory_changes", "project_index_refresh_generation",
    "project_index_refresh_status", "project_intelligence_revisions", "repository_observations",
    "working_set_file_samples", "working_set_invalidations", "working_set_projections",
    "working_set_publication_tickets",
];

// After every direct owner matches, declared FKs establish same-project child
// ownership. These additional edges are intentionally absent from legacy DDL.
const REQUIRED_EDGES: &[&str] = &[
    "SELECT 1 FROM capture_current_words AS child LEFT JOIN blackboard_entries AS owner ON owner.id=child.entry_id LEFT JOIN blackboard_entry_revisions AS revision ON revision.entry_id=child.entry_id AND revision.revision=child.revision WHERE revision.entry_id IS NULL OR child.revision<>owner.revision",
    "SELECT 1 FROM capture_group_members AS child LEFT JOIN blackboard_entry_revisions AS owner ON owner.entry_id=child.entry_id AND owner.revision=child.revision WHERE (child.entry_id IS NULL)<>(child.revision IS NULL) OR (child.entry_id IS NOT NULL AND owner.entry_id IS NULL) OR (child.entry_id IS NULL AND child.outcome NOT IN ('omitted','failed','not_restored'))",
    "SELECT 1 FROM capture_action_outcomes AS child LEFT JOIN blackboard_entry_revisions AS owner ON owner.entry_id=child.entry_id AND owner.revision=child.revision WHERE owner.entry_id IS NULL",
    "SELECT 1 FROM memory_changes AS child LEFT JOIN blackboard_entry_revisions AS owner ON owner.entry_id=child.entry_id AND owner.revision=child.revision WHERE (child.entry_id IS NULL)<>(child.revision IS NULL) OR (child.entry_id IS NOT NULL AND owner.entry_id IS NULL)",
];

pub(super) async fn inspect_on(
    connection: &mut SqliteConnection,
    identity: StoreIdentity,
) -> Result<ProjectKnowledgeSnapshot, ProjectKnowledgeReadError> {
    let invalid_clock: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM project_intelligence_revisions WHERE typeof(revision)<>'integer' OR revision<0)
        OR EXISTS(SELECT 1 FROM capsule_eligibility_generations WHERE typeof(generation)<>'integer' OR generation<=0)
        OR EXISTS(SELECT 1 FROM blackboard_entries WHERE typeof(revision)<>'integer' OR revision<=0 OR typeof(created_at_ms)<>'integer' OR created_at_ms<0 OR typeof(updated_at_ms)<>'integer' OR updated_at_ms<0)
        OR EXISTS(SELECT 1 FROM blackboard_entry_revisions WHERE typeof(revision)<>'integer' OR revision<=0)",
    ).fetch_one(&mut *connection).await?;
    if invalid_clock {
        return Err(ProjectKnowledgeReadError::InvalidClock);
    }
    let missing_revision: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM blackboard_entries AS entry LEFT JOIN blackboard_entry_revisions AS revision ON revision.entry_id=entry.id AND revision.revision=entry.revision WHERE revision.entry_id IS NULL)",
    ).fetch_one(&mut *connection).await?;
    if missing_revision {
        return Err(ProjectKnowledgeReadError::InvalidEntry);
    }
    let project = identity.project_uuid.to_string();
    for table in PROJECT_TABLES {
        // Identifiers are compiled constants; no caller or database text is SQL.
        let sql = format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE typeof(project_id)<>'text' OR project_id<>?)");
        let foreign: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .bind(&project).fetch_one(&mut *connection).await?;
        if foreign {
            return Err(ProjectKnowledgeReadError::InvalidOwnership);
        }
    }
    let broken_fk: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)")
        .fetch_one(&mut *connection).await?;
    if broken_fk {
        return Err(ProjectKnowledgeReadError::InvalidOwnership);
    }
    for edge in REQUIRED_EDGES {
        let sql = format!("SELECT EXISTS({edge})");
        let broken: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .fetch_one(&mut *connection).await?;
        if broken {
            return Err(ProjectKnowledgeReadError::InvalidOwnership);
        }
    }
    let cyclic: bool = sqlx::query_scalar(
        "WITH RECURSIVE reach(start,id) AS (SELECT id,parent_id FROM hierarchy_nodes WHERE parent_id IS NOT NULL UNION SELECT reach.start,node.parent_id FROM reach JOIN hierarchy_nodes AS node ON node.id=reach.id WHERE node.parent_id IS NOT NULL) SELECT EXISTS(SELECT 1 FROM reach WHERE start=id)",
    ).fetch_one(&mut *connection).await?;
    if cyclic {
        return Err(ProjectKnowledgeReadError::InvalidOwnership);
    }
    let (revision, generation): (i64, i64) = sqlx::query_as(
        "SELECT COALESCE((SELECT revision FROM project_intelligence_revisions WHERE project_id=?),0), COALESCE((SELECT generation FROM capsule_eligibility_generations WHERE project_id=?),0)",
    ).bind(&project).bind(&project).fetch_one(&mut *connection).await?;
    let coverage_complete = super::super::identity::coverage_available(connection, &project).await?;
    Ok(ProjectKnowledgeSnapshot {
        identity,
        project_revision: revision as u64,
        eligibility_generation: generation as u64,
        coverage_complete,
    })
}
