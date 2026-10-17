use codex_project_store::PreparedDomains;
use codex_project_store::ProjectStoreLocation;
use codex_project_store::StoreIdentity;
use codex_project_store::initialize_local_store;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use sqlx::Connection;
use sqlx::SqliteConnection;
use sqlx::sqlite::SqliteConnectOptions;
use std::path::Path;
use std::process::Command;
use std::process::Output;
use tempfile::TempDir;

#[expect(
    clippy::disallowed_methods,
    reason = "explicit disposable project fixture"
)]
async fn seed(
    project: &Path,
    content: &str,
) -> Result<(StoreIdentity, SqliteConnection), Box<dyn std::error::Error>> {
    let location =
        ProjectStoreLocation::select_project(&AbsolutePathBuf::from_absolute_path(project)?)?;
    let identity = initialize_local_store(&location).await?;
    let domains = PreparedDomains::prepare(&[
        codex_project_intelligence::project_store_domain(),
        codex_stateful_runtime::project_store_domain()?,
    ])
    .await?;
    let mut writer = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(location.database_path().as_path())
            .foreign_keys(/*on*/ true),
    )
    .await?;
    domains.install(&mut writer).await?;
    let project_id = identity.project_uuid.to_string();
    sqlx::query("INSERT INTO hierarchy_nodes(id,project_id,parent_id,kind,project_root,relative_path,lifecycle,revision,created_at_ms,updated_at_ms) VALUES ('node',?,NULL,'project',NULL,'','active',1,0,0)").bind(&project_id).execute(&mut writer).await?;
    sqlx::query("INSERT INTO blackboard_entries VALUES ('entry',?,'node',1,0,0)")
        .bind(&project_id)
        .execute(&mut writer)
        .await?;
    sqlx::query("INSERT INTO blackboard_entry_revisions(entry_id,revision,kind,content,confidence_basis_points,verification,importance,root_promotion,state,provenance_kind,provenance_source_id,recorded_at_ms) VALUES ('entry',1,'note',?,5000,'unverified','normal','not_promoted','active','user','synthetic',0)").bind(content).execute(&mut writer).await?;
    sqlx::query("UPDATE project_intelligence_revisions SET revision=9007199254740993")
        .execute(&mut writer)
        .await?;
    sqlx::query("UPDATE capsule_eligibility_generations SET generation=9007199254740995")
        .execute(&mut writer)
        .await?;
    Ok((identity, writer))
}

fn read(project: &Path, identity: &StoreIdentity) -> Result<Output, Box<dyn std::error::Error>> {
    Ok(
        Command::new(codex_utils_cargo_bin::cargo_bin("stateful-memory")?)
            .arg("--project")
            .arg(project)
            .args(["--json", "read"])
            .arg("--project-uuid")
            .arg(identity.project_uuid.to_string())
            .arg("--store-uuid")
            .arg(identity.store_uuid.to_string())
            .args(["--entry", "entry"])
            .env_remove("OPENAI_API_KEY")
            .env_remove("CODEX_API_KEY")
            .env("CODEX_HOME", project.join("unused-home"))
            .output()?,
    )
}

#[tokio::test]
async fn auth_free_subprocess_reads_two_projects_and_roundtrips_escaped_content() {
    for content in [
        "alpha \"quoted\"\n\u{092e}\u{1f642}",
        "beta contradicts alpha",
    ] {
        let project = TempDir::new().unwrap();
        let (identity, mut writer) = seed(project.path(), content).await.unwrap();
        let output = read(project.path(), &identity).unwrap();
        assert!(output.status.success(), "{:?}", output.stderr);
        assert!(
            output.stderr.is_empty() && output.stdout.is_ascii() && output.stdout.len() <= 8192
        );
        let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            reply,
            json!({"ok":true,"result":{
            "projectUuid":identity.project_uuid.to_string(),"storeUuid":identity.store_uuid.to_string(),
            "snapshotRevision":"9007199254740993","eligibilityGeneration":"9007199254740995","coverage":"complete","projection":"attributedContent",
            "verification":"notEvaluated","omissions":["structuredValue","evidence","premises","relations","context","applicability"],
            "entry":{"id":"entry","nodeId":"node","revision":"1","createdAtMs":"0","updatedAtMs":"0","kind":"note","state":"active","content":content,"provenance":{"kind":"user","sourceId":"synthetic"}}}})
        );
        if content.starts_with("alpha") {
            let mut discovery =
                Command::new(codex_utils_cargo_bin::cargo_bin("stateful-memory").unwrap());
            discovery
                .arg("--project")
                .arg(project.path())
                .args(["--json", "capabilities"]);
            let cap_output = discovery.output().unwrap();
            assert!(cap_output.status.success() && cap_output.stderr.is_empty());
            assert!(
                cap_output.stdout.is_ascii()
                    && cap_output.stdout.len() <= 4096
                    && cap_output.stdout.last() == Some(&b'\n')
            );
            let cap: Value = serde_json::from_slice(&cap_output.stdout).unwrap();
            let labels = ["projection", "verification", "omissions"];
            assert_eq!(
                labels.map(|field| &cap["result"]["contentRead"][field]),
                labels.map(|field| &reply["result"][field]),
            );
            let mut normalized = reply;
            // JSON object order differs with serde_json's preserve_order feature.
            // Snapshot the complete parsed reply; check its wire envelope above.
            normalized["capabilities"] = cap;
            normalized["result"]["projectUuid"] = json!("[project-uuid]");
            normalized["result"]["storeUuid"] = json!("[store-uuid]");
            normalized.sort_all_objects();
            insta::assert_snapshot!(
                "attributed_read",
                serde_json::to_string_pretty(&normalized).unwrap()
            );
        }
        assert!(!project.path().join("unused-home").exists());
        sqlx::query("UPDATE blackboard_entry_revisions SET state='tombstoned'")
            .execute(&mut writer)
            .await
            .unwrap();
        let mut missing: Value = serde_json::from_slice(&output.stdout).unwrap();
        missing["result"]["entry"] = Value::Null;
        missing["result"]["eligibilityGeneration"] = json!("9007199254740996");
        let retired = read(project.path(), &identity).unwrap();
        assert!(retired.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&retired.stdout).unwrap(),
            missing
        );
        writer.close().await.unwrap();
    }
}

#[tokio::test]
async fn escaped_reply_overflow_refuses_without_delivering_partial_content() {
    let project = TempDir::new().unwrap();
    let (identity, writer) = seed(project.path(), &"\u{0001}".repeat(/*n*/ 2048))
        .await
        .unwrap();
    let output = read(project.path(), &identity).unwrap();
    assert!(!output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!({"ok":false,"error":{"code":"responseTooLarge","message":"Reply cannot fit the 8192-byte ASCII envelope."}})
    );
    assert!(output.stdout.is_ascii() && output.stdout.len() < 8192 && output.stderr.is_empty());
    writer.close().await.unwrap();
}
