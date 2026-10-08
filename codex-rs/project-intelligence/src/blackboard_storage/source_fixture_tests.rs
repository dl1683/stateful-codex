use crate::SourceObservation;
use crate::SourceSpan;
use crate::SourceSpanRole;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use tempfile::TempDir;
const PROJECT: &str = "project-1";

pub(super) fn observation(event: &str, text: &str) -> SourceObservation {
    SourceObservation {
        project_id: PROJECT.to_string(),
        authoritative_thread_id: "00000000-0000-0000-0000-000000000001".to_string(),
        binding_generation: 1,
        original_event_id: event.to_string(),
        turn_id: "turn-1".to_string(),
        part_index: 0,
        source_revision: 1,
        complete_envelope: text.len() <= 16384,
        incomplete_reason: (text.len() > 16384).then(|| "envelope too large".to_string()),
        ordered_spans: if text.len() <= 16384 {
            vec![SourceSpan {
                start_byte: 0,
                end_byte: text.len() as u32,
                role: SourceSpanRole::Body,
            }]
        } else {
            Vec::new()
        },
    }
}

pub(super) async fn admission(
    home: &TempDir,
) -> std::sync::Arc<codex_state::ThreadProjectAdmission> {
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let pool = sqlite
        .open_read_write_pool(&sqlite.state_db_path())
        .await
        .unwrap();
    sqlx::query("CREATE TABLE IF NOT EXISTS threads(id TEXT PRIMARY KEY, project_id TEXT, project_binding_generation INTEGER NOT NULL DEFAULT 1)").execute(&pool).await.unwrap();
    sqlx::query("INSERT OR IGNORE INTO threads(id, project_id) VALUES ('00000000-0000-0000-0000-000000000001', 'project-1')").execute(&pool).await.unwrap();
    let thread = serde_json::from_str("\"00000000-0000-0000-0000-000000000001\"").unwrap();
    std::sync::Arc::new(
        codex_state::ThreadProjectAdmission::acquire(&sqlite, thread, PROJECT)
            .await
            .unwrap()
            .unwrap(),
    )
}
