use std::path::Path;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;

use codex_project_store::ProjectStoreLocation;
use codex_project_store::StoreIdentity;
use codex_project_store::initialize_local_store;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

fn command(project: &Path, operation: &str) -> Command {
    let mut command = Command::new(codex_utils_cargo_bin::cargo_bin("stateful-memory").unwrap());
    command
        .arg("--project")
        .arg(project)
        .args([operation, "--json"]);
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
    let first = successful_reply(command(project.path(), "root").output().unwrap());
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
    let missing = command(project.path(), "root").output().unwrap();
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
    let refused = command(project.path(), "root").output().unwrap();
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
    let first = successful_reply(command(&unicode, "root").output().unwrap());
    let second = successful_reply(command(&other, "root").output().unwrap());
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

#[test]
fn independent_initializers_publish_one_identity_and_retry_without_replacement() {
    let project = TempDir::new().unwrap();
    let first = command(project.path(), "init")
        .args(["--storage", "local-unsynchronized"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let second = command(project.path(), "init")
        .args(["--storage", "local-unsynchronized"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let initialized = successful_reply(first.wait_with_output().unwrap());
    assert_eq!(
        successful_reply(second.wait_with_output().unwrap()),
        initialized
    );
    let database = project.path().join(".stateful/memory.sqlite");
    let bytes = std::fs::read(&database).unwrap();
    let orphan = project.path().join(".stateful/.bootstrap-abandoned.sqlite");
    std::fs::write(&orphan, b"incomplete ignored stage").unwrap();
    assert_eq!(
        successful_reply(
            command(project.path(), "init")
                .args(["--storage", "local-unsynchronized"])
                .output()
                .unwrap()
        ),
        initialized
    );
    assert_eq!(
        successful_reply(command(project.path(), "root").output().unwrap()),
        initialized
    );
    assert_eq!(std::fs::read(database).unwrap(), bytes);
    assert_eq!(std::fs::read(orphan).unwrap(), b"incomplete ignored stage");
    assert!(!project.path().join("unused-engine-home").exists());
    let mut normalized = initialized;
    for (field, replacement) in [
        ("projectUuid", "[project-uuid]"),
        ("storeUuid", "[store-uuid]"),
        ("projectRoot", "[project-root]"),
        ("databasePath", "[database-path]"),
    ] {
        normalized["result"][field] = replacement.into();
    }
    insta::assert_snapshot!(
        "bootstrap_initialized",
        serde_json::to_string_pretty(&normalized).unwrap()
    );
}

#[test]
fn initialization_requires_storage_qualification_and_refuses_foreign_files() {
    let project = TempDir::new().unwrap();
    let missing = command(project.path(), "init").output().unwrap();
    assert!(!missing.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&missing.stdout).unwrap()["error"]["code"],
        "invalidArguments"
    );
    assert!(!project.path().join(".stateful").exists());
    std::fs::create_dir(project.path().join(".stateful")).unwrap();
    let database = project.path().join(".stateful/memory.sqlite");
    std::fs::write(&database, b"").unwrap();
    let foreign = command(project.path(), "init")
        .args(["--storage", "local-unsynchronized"])
        .output()
        .unwrap();
    assert!(!foreign.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&foreign.stdout).unwrap()["error"]["code"],
        "unsupportedStore"
    );
    assert_eq!(std::fs::read(database).unwrap(), b"");
}
