use std::path::Path;
use std::process::Command;

use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use sqlx::Connection;
use sqlx::SqliteConnection;
use sqlx::sqlite::SqliteConnectOptions;
use tempfile::TempDir;

#[expect(
    clippy::disallowed_methods,
    reason = "disposable native-layout fixture"
)]
async fn seed(project: &Path, content: &str) -> SqliteConnection {
    let root = project.join(".stateful");
    std::fs::create_dir(&root).unwrap();
    let mut writer = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(root.join("project_intelligence_1.sqlite"))
            .create_if_missing(/*create*/ true),
    )
    .await
    .unwrap();
    codex_project_intelligence::project_store_domain()
        .migrations
        .run(&mut writer)
        .await
        .unwrap();
    sqlx::query("INSERT INTO hierarchy_nodes(id,project_id,parent_id,kind,project_root,relative_path,lifecycle,revision,created_at_ms,updated_at_ms) VALUES ('node','project-1',NULL,'project',NULL,'','active',1,0,0)").execute(&mut writer).await.unwrap();
    sqlx::query("INSERT INTO blackboard_entries VALUES ('entry','project-1','node',1,0,0)")
        .execute(&mut writer)
        .await
        .unwrap();
    sqlx::query("INSERT INTO blackboard_entry_revisions(entry_id,revision,kind,content,confidence_basis_points,verification,importance,root_promotion,state,provenance_kind,provenance_source_id,recorded_at_ms) VALUES ('entry',1,'note',?,5000,'unverified','normal','not_promoted','active','user','synthetic',0)").bind(content).execute(&mut writer).await.unwrap();
    writer
}

fn command(project: &Path, operation: &str) -> Command {
    let mut command = Command::new(codex_utils_cargo_bin::cargo_bin("stateful-memory").unwrap());
    command
        .arg("--project")
        .arg(project)
        .args(["--json", operation])
        .env_remove("STATEFUL_STORE_ROOT")
        .env("CODEX_HOME", project.join("unused-home"));
    command
}

fn read(project: &Path, project_id: &str) -> Value {
    let output = command(project, "native-read")
        .args(["--project-id", project_id, "--entry", "entry"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.len() <= 8192 && output.stdout.is_ascii());
    serde_json::from_slice(&output.stdout).unwrap()
}

#[tokio::test]
async fn native_subprocess_reads_isolated_projects_after_restart_and_honors_retirement() {
    for content in ["alpha \"quoted\"\nमराठी 🙂", "beta contradicts alpha"] {
        let project = TempDir::new().unwrap();
        let mut writer = seed(project.path(), content).await;
        let before: Vec<String> =
            sqlx::query_scalar("SELECT name FROM sqlite_master ORDER BY name")
                .fetch_all(&mut writer)
                .await
                .unwrap();
        let expected = json!({"ok":true,"result":{"layout":"nativeHome","readOnly":true,"entry":{
            "projectId":"project-1","id":"entry","revision":"1","content":content,
            "state":"active","kind":"note",
            "provenance":"user","sourceId":"synthetic","effectiveVerification":"unverified",
            "evidenceFreshness":"notApplicable","premiseFreshness":"notApplicable",
            "omissions":["structuredValue","context","evidence","premises","relations"]
        }}});
        assert_eq!(read(project.path(), "project-1"), expected);
        assert_eq!(read(project.path(), "project-1"), expected);
        assert_eq!(
            read(project.path(), "wrong-project"),
            json!({"ok":true,"result":{"layout":"nativeHome","readOnly":true,"entry":null}})
        );
        sqlx::query("UPDATE blackboard_entry_revisions SET state='tombstoned'")
            .execute(&mut writer)
            .await
            .unwrap();
        assert_eq!(
            read(project.path(), "project-1"),
            json!({"ok":true,"result":{"layout":"nativeHome","readOnly":true,"entry":null}})
        );
        let after: Vec<String> = sqlx::query_scalar("SELECT name FROM sqlite_master ORDER BY name")
            .fetch_all(&mut writer)
            .await
            .unwrap();
        assert_eq!(before, after);
        assert!(!project.path().join("unused-home").exists());
        assert!(!project.path().join(".stateful/memory.sqlite").exists());
        let output = command(project.path(), "root").output().unwrap();
        assert!(output.status.success());
        let mut reply: Value = serde_json::from_slice(&output.stdout).unwrap();
        reply["result"]["databasePath"] = "[native-database]".into();
        reply.sort_all_objects();
        insta::assert_snapshot!("native_root", serde_json::to_string_pretty(&reply).unwrap());
        writer.close().await.unwrap();
        std::fs::rename(
            project.path().join(".stateful"),
            project.path().join("alternate"),
        )
        .unwrap();
        let output = command(project.path(), "native-root")
            .args(["--store-root", "alternate"])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(!project.path().join(".stateful").exists());
    }
}

#[test]
fn native_missing_store_is_not_created() {
    let project = TempDir::new().unwrap();
    let output = command(project.path(), "native-root").output().unwrap();
    assert!(!output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["error"]["code"],
        "storeUnavailable"
    );
    assert!(!project.path().join(".stateful").exists());
}
