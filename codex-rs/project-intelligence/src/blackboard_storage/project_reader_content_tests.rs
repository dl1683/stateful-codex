use super::fixture;
use crate::blackboard_storage::ProjectKnowledgeReadError;
use codex_project_store::StoreIdentity;
use pretty_assertions::assert_eq;
use serde_json::json;
use sqlx::Connection;
use sqlx::SqliteConnection;
use tempfile::TempDir;

async fn seed(writer: &mut SqliteConnection, identity: &StoreIdentity, content: &str) {
    let project = identity.project_uuid.to_string();
    sqlx::query("INSERT INTO hierarchy_nodes(id,project_id,parent_id,kind,project_root,relative_path,lifecycle,revision,created_at_ms,updated_at_ms) VALUES ('node',?,NULL,'project',NULL,'','active',1,0,0)")
        .bind(&project).execute(&mut *writer).await.unwrap();
    sqlx::query("INSERT INTO blackboard_entries VALUES ('entry',?,'node',9007199254740993,9007199254740995,9007199254740997)")
        .bind(&project).execute(&mut *writer).await.unwrap();
    // Synthetic scalar fixture, not authenticated user ingress or verified truth.
    sqlx::query("INSERT INTO blackboard_entry_revisions(entry_id,revision,kind,content,confidence_basis_points,verification,importance,root_promotion,state,provenance_kind,provenance_source_id,recorded_at_ms) VALUES ('entry',9007199254740993,'note',?,5000,'unverified','normal','not_promoted','active','user','synthetic',0)")
        .bind(content).execute(&mut *writer).await.unwrap();
    sqlx::query("INSERT INTO project_intelligence_revisions VALUES (?,9007199254740993) ON CONFLICT(project_id) DO UPDATE SET revision=excluded.revision").bind(&project).execute(&mut *writer).await.unwrap();
    sqlx::query("INSERT INTO capsule_eligibility_generations VALUES (?,9007199254740995) ON CONFLICT(project_id) DO UPDATE SET generation=excluded.generation").bind(&project).execute(writer).await.unwrap();
}

fn expected(identity: &StoreIdentity, content: &str, generation: u64) -> serde_json::Value {
    json!({"projectUuid":identity.project_uuid.to_string(),"storeUuid":identity.store_uuid.to_string(),
        "snapshotRevision":"9007199254740993","eligibilityGeneration":generation.to_string(),
        "coverage":"complete","projection":"attributedContent","verification":"notEvaluated",
        "omissions":["structuredValue","evidence","premises","relations","context","applicability"],
        "entry":{"id":"entry","nodeId":"node","revision":"9007199254740993","createdAtMs":"9007199254740995","updatedAtMs":"9007199254740997",
            "kind":"note","state":"active","content":content,"provenance":{"kind":"user","sourceId":"synthetic"}}})
}

#[tokio::test]
async fn identical_ids_in_distinct_stores_return_exact_attributed_content_and_lossless_clocks() {
    for content in ["alpha \"quoted\" \n \u{092e}", "beta contradicts alpha"] {
        let home = TempDir::new().unwrap();
        let (mut reader, identity, mut writer) = fixture(&home).await;
        seed(&mut writer, &identity, content).await;
        // Stored verification and unsupported nested payloads do not become a certificate.
        sqlx::query("UPDATE blackboard_entry_revisions SET verification='source_verified',structured_value=zeroblob(1000000)").execute(&mut writer).await.unwrap();
        assert_eq!(
            serde_json::to_value(reader.read(&identity, "entry").await.unwrap()).unwrap(),
            expected(&identity, content, /*generation*/ 9007199254740996)
        );
        let mut missing = expected(&identity, content, /*generation*/ 9007199254740996);
        missing["entry"] = serde_json::Value::Null;
        assert_eq!(
            serde_json::to_value(reader.read(&identity, "missing").await.unwrap()).unwrap(),
            missing
        );
        reader.close().await.unwrap();
        writer.close().await.unwrap();
    }
}

#[tokio::test]
async fn refusal_precedence_and_ordinary_reuse_preserve_whole_store_admission() {
    let home = TempDir::new().unwrap();
    let (mut reader, identity, mut writer) = fixture(&home).await;
    seed(&mut writer, &identity, "alpha").await;
    sqlx::query("UPDATE project_intelligence_revisions SET revision='bad'")
        .execute(&mut writer)
        .await
        .unwrap();
    assert!(matches!(
        reader.read(&identity, &"x".repeat(/*n*/ 513)).await,
        Err(ProjectKnowledgeReadError::InvalidRequest)
    ));
    let mut wrong = identity.clone();
    wrong.store_uuid = Default::default();
    assert!(matches!(
        reader.read(&wrong, "entry").await,
        Err(ProjectKnowledgeReadError::IdentityMismatch)
    ));
    assert!(matches!(
        reader.read(&identity, "entry").await,
        Err(ProjectKnowledgeReadError::InvalidClock)
    ));
    sqlx::query("UPDATE project_intelligence_revisions SET revision=9007199254740993")
        .execute(&mut writer)
        .await
        .unwrap();
    sqlx::query("INSERT INTO capture_projection_coverage(project_id) VALUES ('foreign')")
        .execute(&mut writer)
        .await
        .unwrap();
    assert!(matches!(
        reader.read(&identity, "missing").await,
        Err(ProjectKnowledgeReadError::InvalidOwnership)
    ));
    sqlx::query("DELETE FROM capture_projection_coverage")
        .execute(&mut writer)
        .await
        .unwrap();
    sqlx::query("INSERT INTO capture_identity_coverage(project_id,watermark) VALUES (?,0)")
        .bind(identity.project_uuid.to_string())
        .execute(&mut writer)
        .await
        .unwrap();
    assert!(matches!(
        reader.read(&identity, "missing").await,
        Err(ProjectKnowledgeReadError::CoverageIncomplete)
    ));
    sqlx::query("DELETE FROM capture_identity_coverage")
        .execute(&mut writer)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(reader.read(&identity, "entry").await.unwrap()).unwrap(),
        expected(&identity, "alpha", /*generation*/ 9007199254740997)
    );
    reader.close().await.unwrap();
    writer.close().await.unwrap();
}

#[tokio::test]
async fn every_call_refreshes_retirement_and_scalar_refusals_finish_before_reuse_and_close() {
    let home = TempDir::new().unwrap();
    let (mut reader, identity, mut writer) = fixture(&home).await;
    seed(&mut writer, &identity, "retained archive").await;
    let mut expected = expected(
        &identity,
        "retained archive",
        /*generation*/ 9007199254740995,
    );
    assert_eq!(
        serde_json::to_value(reader.read(&identity, "entry").await.unwrap()).unwrap(),
        expected
    );
    sqlx::query("UPDATE blackboard_entry_revisions SET state='tombstoned'")
        .execute(&mut writer)
        .await
        .unwrap();
    expected["eligibilityGeneration"] = json!("9007199254740996");
    let mut missing = expected.clone();
    missing["entry"] = serde_json::Value::Null;
    assert_eq!(
        serde_json::to_value(reader.read(&identity, "entry").await.unwrap()).unwrap(),
        missing
    );
    let archive: String = sqlx::query_scalar("SELECT content FROM blackboard_entry_revisions")
        .fetch_one(&mut writer)
        .await
        .unwrap();
    assert_eq!(archive, "retained archive");
    sqlx::query("UPDATE blackboard_entry_revisions SET state='active',content=zeroblob(2049)")
        .execute(&mut writer)
        .await
        .unwrap();
    assert!(matches!(
        reader.read(&identity, "entry").await,
        Err(ProjectKnowledgeReadError::UnsupportedContent)
    ));
    sqlx::query("UPDATE blackboard_entry_revisions SET content='retained archive'")
        .execute(&mut writer)
        .await
        .unwrap();
    expected["eligibilityGeneration"] = json!("9007199254740998");
    assert_eq!(
        serde_json::to_value(reader.read(&identity, "entry").await.unwrap()).unwrap(),
        expected
    );
    reader.close().await.unwrap();
    writer.close().await.unwrap();
    let moved = TempDir::new().unwrap();
    std::fs::rename(
        home.path().join(".stateful"),
        moved.path().join(".stateful"),
    )
    .unwrap();
}
