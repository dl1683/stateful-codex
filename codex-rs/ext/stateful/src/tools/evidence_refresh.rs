//! Targeted indexing for an explicitly named source file.
//!
//! Reading one file never waits for a whole-project scan: the named path is resolved
//! against the configured roots by canonical containment and indexed on its own, even
//! when the project was never indexed or the file is ignored by the corpus scan. Every
//! outcome has a precise status, so an existing readable file is never reported as
//! missing merely because an index row was absent.

use std::path::Path;
use std::path::PathBuf;

use codex_extension_api::FunctionCallError;
use codex_project_intelligence::ProjectIndexFileRequest;
use codex_project_intelligence::ProjectIndexer;
use codex_project_intelligence::ProjectIndexerError;
use codex_project_intelligence::ProjectRelativePath;

use crate::index_gate::EXPLICIT_FILE_DEADLINE;
use crate::index_gate::IndexOperation;
use crate::services::ProjectIntelligenceServices;

const READ_DIRECTLY: &str = "Read the file directly instead; an evidence receipt is optional.";

/// The configured root an explicit path belongs to: the requested root, or the only root
/// in which the path exists. A path that exists under several roots needs `projectRoot`.
pub(super) async fn source_root(
    project_roots: &[PathBuf],
    requested_root: Option<&PathBuf>,
    relative_path: &ProjectRelativePath,
) -> Result<PathBuf, FunctionCallError> {
    let candidates = match requested_root {
        Some(root) if project_roots.contains(root) => vec![root.clone()],
        Some(root) => {
            return Err(status(
                "rootOutsideProject",
                format!(
                    "{} is not a root of the selected project; roots: {}",
                    root.display(),
                    list_roots(project_roots)
                ),
            ));
        }
        None => project_roots.to_vec(),
    };
    let path = relative_path.as_str().to_string();
    let lookup = candidates.clone();
    let existing = tokio::task::spawn_blocking(move || {
        lookup
            .into_iter()
            .filter(|root| contains_file(root, &path))
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|error| status("indexUnavailable", error.to_string()))?;
    match (existing.as_slice(), candidates.as_slice()) {
        ([root], _) => Ok(root.clone()),
        // A single root is refreshed even when the file is gone, so an indexed row that
        // outlived its file is marked missing rather than read.
        ([], [root]) => Ok(root.clone()),
        ([], _) => Err(status(
            "notFound",
            format!(
                "{relative_path} does not exist under any project root ({})",
                list_roots(&candidates)
            ),
        )),
        _ => Err(status(
            "ambiguousRoot",
            format!(
                "{relative_path} exists under several project roots; provide projectRoot as one of: {}",
                list_roots(&existing)
            ),
        )),
    }
}

/// Indexes just this file under the project's index permit, within the foreground
/// deadline for explicit reads.
pub(super) async fn refresh_explicit_file(
    services: &ProjectIntelligenceServices,
    project_id: &str,
    project_root: PathBuf,
    relative_path: ProjectRelativePath,
) -> Result<(), FunctionCallError> {
    let indexer = ProjectIndexer::new(
        services
            .hierarchy()
            .await
            .map_err(|error| status("indexUnavailable", error.to_string()))?
            .clone(),
        services
            .context_map()
            .await
            .map_err(|error| status("indexUnavailable", error.to_string()))?
            .clone(),
    );
    let request = ProjectIndexFileRequest {
        project_id: project_id.to_string(),
        project_root,
        relative_path: relative_path.clone(),
    };
    match services
        .index_gates()
        .run(project_id, EXPLICIT_FILE_DEADLINE, move |_| async move {
            indexer.refresh_file(request).await
        })
        .await
    {
        IndexOperation::Finished(Ok(_)) => Ok(()),
        IndexOperation::Finished(Err(ProjectIndexerError::SourceNotIndexed(_))) => Err(status(
            "notFound",
            format!("{relative_path} does not exist in the project root"),
        )),
        IndexOperation::Finished(Err(ProjectIndexerError::InvalidRoot)) => Err(status(
            "notFound",
            format!("{relative_path} is not a regular file inside its project root"),
        )),
        IndexOperation::Finished(Err(error)) => Err(status(
            "indexUnavailable",
            format!("indexing {relative_path} failed ({error}). {READ_DIRECTLY}"),
        )),
        IndexOperation::Pending => Err(status(
            "timedOutWorkPending",
            format!(
                "indexing {relative_path} is still running and was not abandoned; retry shortly. {READ_DIRECTLY}"
            ),
        )),
        IndexOperation::Failed(error) => Err(status(
            "indexUnavailable",
            format!("indexing {relative_path} stopped unexpectedly ({error}). {READ_DIRECTLY}"),
        )),
    }
}

fn contains_file(root: &Path, relative_path: &str) -> bool {
    let (Ok(root), Ok(path)) = (
        std::fs::canonicalize(root),
        std::fs::canonicalize(root.join(relative_path)),
    ) else {
        return false;
    };
    path.starts_with(&root) && path.is_file()
}

fn list_roots(roots: &[PathBuf]) -> String {
    roots
        .iter()
        .map(|root| root.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A model-facing failure that leads with its machine-readable status.
pub(super) fn status(code: &str, message: impl std::fmt::Display) -> FunctionCallError {
    FunctionCallError::RespondToModel(format!("{code}: {message}"))
}
