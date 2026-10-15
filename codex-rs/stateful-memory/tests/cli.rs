use std::path::Path;
use std::process::Command;
use std::process::Output;

use codex_project_store::ProjectStoreLocation;
use codex_project_store::StoreIdentity;
use codex_project_store::initialize_local_store;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

fn command(project: &Path) -> Command {
    let mut command = Command::new(codex_utils_cargo_bin::cargo_bin("stateful-memory").unwrap());
    command
        .arg("--project")
        .arg(project)
        .args(["root", "--json"]);
    command
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_API_KEY")
        .env("CODEX_HOME", project.join("unused-engine-home"));
    command
}

fn successful_reply(output: Output) -> Value {
    assert!(
        output.status.success(),
        "stdout={:?}, stderr={:?}",
        output.stdout,
        output.stderr
    );
    assert!(output.stdout.len() <= 4096 && output.stdout.is_ascii());
    serde_json::from_slice(&output.stdout).unwrap()
}

async fn seed(project: &Path) -> (ProjectStoreLocation, StoreIdentity) {
    let root = AbsolutePathBuf::from_absolute_path(project).unwrap();
    let location = ProjectStoreLocation::select_project(&root).unwrap();
    let identity = initialize_local_store(&location).await.unwrap();
    (location, identity)
}

#[tokio::test]
async fn fresh_reader_returns_the_project_identity_without_an_engine_home() {
    let project = TempDir::new().unwrap();
    let (location, identity) = seed(project.path()).await;
    let first = successful_reply(command(project.path()).output().unwrap());
    assert_eq!(
        first,
        json!({"ok":true,"result":{
            "projectUuid":identity.project_uuid.to_string(),"storeUuid":identity.store_uuid.to_string(),
            "projectRoot":location.project_root().as_path().to_str().unwrap(),
            "databasePath":location.database_path().as_path().to_str().unwrap(),
            "coverage":"bootstrapOnly","knowledgeAvailable":false,"nativeAttached":false,
            "snapshotRevision":null,"eligibilityGeneration":null
        }})
    );
    assert!(!project.path().join("unused-engine-home").exists());
    let mut normalized = first;
    for (field, replacement) in [
        ("projectUuid", "[project-uuid]"),
        ("storeUuid", "[store-uuid]"),
        ("projectRoot", "[project-root]"),
        ("databasePath", "[database-path]"),
    ] {
        normalized["result"][field] = replacement.into();
    }
    insta::assert_snapshot!(
        "bootstrap_root",
        serde_json::to_string_pretty(&normalized).unwrap()
    );
}

#[test]
fn absent_and_foreign_reads_refuse_without_creating_or_replacing_authority() {
    let project = TempDir::new().unwrap();
    let missing = command(project.path()).output().unwrap();
    assert!(!missing.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&missing.stdout).unwrap()["error"]["code"],
        "storeUnavailable"
    );
    assert!(!project.path().join(".stateful").exists());
    assert!(!project.path().join("unused-engine-home").exists());
    std::fs::create_dir(project.path().join(".stateful")).unwrap();
    let foreign = project.path().join(".stateful/memory.sqlite");
    std::fs::write(&foreign, b"").unwrap();
    let refused = command(project.path()).output().unwrap();
    assert!(!refused.status.success());
    assert!(refused.stdout.len() <= 4096 && refused.stdout.is_ascii());
    assert_eq!(
        serde_json::from_slice::<Value>(&refused.stdout).unwrap()["error"]["code"],
        "unsupportedStore"
    );
    assert_eq!(std::fs::read(foreign).unwrap(), b"");
}

#[tokio::test]
async fn unicode_paths_roundtrip_and_other_projects_have_distinct_identities() {
    let workspace = TempDir::new().unwrap();
    let unicode = workspace
        .path()
        .join("project-\u{092e}\u{0930}\u{093e}\u{0920}\u{0940}");
    let other = workspace.path().join("other");
    std::fs::create_dir(&unicode).unwrap();
    std::fs::create_dir(&other).unwrap();
    let (location, _) = seed(&unicode).await;
    seed(&other).await;
    let first = successful_reply(command(&unicode).output().unwrap());
    let second = successful_reply(command(&other).output().unwrap());
    assert_eq!(
        first["result"]["projectRoot"],
        location.project_root().as_path().to_str().unwrap()
    );
    assert_ne!(
        first["result"]["projectUuid"],
        second["result"]["projectUuid"]
    );
    assert_ne!(first["result"]["storeUuid"], second["result"]["storeUuid"]);
}
