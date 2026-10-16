//! Neutral, readonly attributed-content operation. No engine home or fallback.
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::ProjectKnowledgeReadError;
use codex_project_intelligence::ProjectKnowledgeReader;
use codex_project_store::DomainManifest;
use codex_project_store::PreparedDomains;
use codex_project_store::ProjectStoreLocation;
use codex_project_store::ProjectStoreOpenError;
use codex_project_store::ProjectStoreReader;
use codex_project_store::StoreIdentity;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde_json::Value;
use std::path::Path;

static INTELLIGENCE: sqlx::migrate::Migrator = sqlx::migrate!("../project-intelligence/migrations");
static RUNTIME: sqlx::migrate::Migrator = sqlx::migrate!("../stateful-runtime/migrations");

fn refused(code: &'static str) -> (&'static str, String) {
    (
        code,
        "Project content read refused; check the error code and project identity.".to_string(),
    )
}

fn read_error(error: ProjectKnowledgeReadError) -> (&'static str, String) {
    refused(match error {
        ProjectKnowledgeReadError::IdentityMismatch => "identityMismatch",
        ProjectKnowledgeReadError::InvalidRequest => "invalidArguments",
        ProjectKnowledgeReadError::InvalidRunStart => "invalidArguments",
        ProjectKnowledgeReadError::ReusableLearningPresent => "reusableLearningPresent",
        ProjectKnowledgeReadError::InvalidClock => "invalidClock",
        ProjectKnowledgeReadError::InvalidEntry => "invalidEntry",
        ProjectKnowledgeReadError::InvalidOwnership => "invalidOwnership",
        ProjectKnowledgeReadError::CoverageIncomplete => "coverageIncomplete",
        ProjectKnowledgeReadError::UnsupportedContent => "unsupportedContent",
        ProjectKnowledgeReadError::Registry(_) => "unsupportedStore",
        ProjectKnowledgeReadError::Snapshot(_) => "snapshotUnavailable",
        ProjectKnowledgeReadError::Sqlite(_) | ProjectKnowledgeReadError::Intelligence(_) => {
            "readUnavailable"
        }
    })
}

pub(super) async fn execute(
    project: &Path,
    project_uuid: &str,
    store_uuid: &str,
    entry: &str,
) -> Result<Value, (&'static str, String)> {
    if project_uuid.len() != 36 || store_uuid.len() != 36 || entry.len() > 512 {
        return Err(refused("invalidArguments"));
    }
    let expected = StoreIdentity {
        project_uuid: project_uuid
            .parse()
            .map_err(|_| refused("invalidArguments"))?,
        store_uuid: store_uuid
            .parse()
            .map_err(|_| refused("invalidArguments"))?,
    };
    if expected.project_uuid.to_string() != project_uuid
        || expected.store_uuid.to_string() != store_uuid
    {
        return Err(refused("invalidArguments"));
    }
    BlackboardEntryId::parse(entry).map_err(|_| refused("invalidArguments"))?;
    let root =
        AbsolutePathBuf::relative_to_current_dir(project).map_err(|_| refused("invalidProject"))?;
    let location =
        ProjectStoreLocation::select_project(&root).map_err(|_| refused("invalidProject"))?;
    let domains = PreparedDomains::prepare(&[
        DomainManifest {
            domain: "project-intelligence",
            migrations: &INTELLIGENCE,
        },
        DomainManifest {
            domain: "stateful-runtime",
            migrations: &RUNTIME,
        },
    ])
    .await
    .map_err(|_| refused("unsupportedStore"))?;
    let store = ProjectStoreReader::open(&location, domains)
        .await
        .map_err(|error| {
            refused(match error {
                ProjectStoreOpenError::Registry(_) => "unsupportedStore",
                ProjectStoreOpenError::Resolve(_) => "invalidProject",
                ProjectStoreOpenError::UnsupportedFilesystem => "unsupportedFilesystem",
                ProjectStoreOpenError::LocationChanged => "locationChanged",
                ProjectStoreOpenError::IgnorePolicy => "ignorePolicyConflict",
                ProjectStoreOpenError::Io(_) | ProjectStoreOpenError::Sql(_) => "storeUnavailable",
            })
        })?;
    let mut reader = ProjectKnowledgeReader::from_project_store(store)
        .await
        .map_err(read_error)?;
    let result = reader.read(&expected, entry).await;
    reader
        .close()
        .await
        .map_err(|_| refused("readUnavailable"))?;
    serde_json::to_value(result.map_err(read_error)?).map_err(|_| refused("readUnavailable"))
}
