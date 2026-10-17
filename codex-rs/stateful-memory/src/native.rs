//! Read-only access to the existing native home layout; no import or fallback.

use std::path::Path;
use std::path::PathBuf;

use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::native_memory_projects;
use codex_project_intelligence::read_native_memory;
use serde_json::Value;
use serde_json::json;

type ReadResult = Result<Value, (&'static str, String)>;

pub(super) fn database(project: &Path, store_root: Option<PathBuf>) -> PathBuf {
    let root = store_root
        .or_else(|| {
            std::env::var_os("STATEFUL_STORE_ROOT")
                .filter(|root| !root.is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| PathBuf::from(".stateful"));
    project.join(root).join("project_intelligence_1.sqlite")
}

fn unavailable() -> (&'static str, String) {
    (
        "storeUnavailable",
        "Native store read failed; database creation and migrations are disabled.".to_string(),
    )
}

pub(super) async fn root(project: &Path, store_root: Option<PathBuf>) -> ReadResult {
    let database = database(project, store_root);
    let rows = native_memory_projects(&database)
        .await
        .map_err(|_| unavailable())?;
    if rows.len() > 20 {
        return Err((
            "responseTooLarge",
            "Native root has more than 20 projects.".to_string(),
        ));
    }
    Ok(json!({
        "layout":"nativeHome", "databasePath":database,
        "readOnly":true, "projects":rows.into_iter().map(|(project_id, node_id)|
            json!({"projectId":project_id,"nodeId":node_id})).collect::<Vec<_>>()
    }))
}

pub(super) async fn read(
    project: &Path,
    store_root: Option<PathBuf>,
    project_id: &str,
    entry: &str,
) -> ReadResult {
    if project_id.is_empty() || project_id.len() > 512 || entry.len() > 512 {
        return Err((
            "invalidArguments",
            "Bounded project and entry identifiers are required.".to_string(),
        ));
    }
    let entry_id = BlackboardEntryId::parse(entry).map_err(|_| {
        (
            "invalidArguments",
            "Invalid native entry identifier.".to_string(),
        )
    })?;
    let hit = read_native_memory(&database(project, store_root), project_id, &entry_id)
        .await
        .map_err(|_| unavailable())?;
    Ok(json!({
        "layout":"nativeHome", "readOnly":true,
        "entry":hit.map(|hit| json!({
            "projectId":project_id,"id":hit.entry.id.to_string(),
            "revision":hit.entry.revision.to_string(),"content":hit.entry.value.content,
            "state":hit.entry.state,"kind":hit.entry.value.kind,
            "provenance":hit.entry.value.provenance.kind,
            "sourceId":hit.entry.value.provenance.source_id,
            "effectiveVerification":hit.effective_verification,
            "evidenceFreshness":hit.evidence_freshness,"premiseFreshness":hit.premise_freshness,
            "omissions":["structuredValue","context","evidence","premises","relations"]
        }))
    }))
}
