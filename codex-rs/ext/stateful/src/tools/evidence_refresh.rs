//! Reading an explicitly named source file, indexing just that file when needed.
//!
//! Reading one file never waits for a whole-project scan. The named path is first
//! resolved against the configured roots by canonical containment, and the read uses that
//! root, so stale or partial index rows cannot make the choice. When the file has no
//! current index entry (a never-indexed project, a new file, or a file the corpus scan
//! ignores), just that file is indexed. The whole operation (resolution, reading,
//! fingerprinting and indexing) runs under the project's index permit with one foreground
//! deadline; past it the caller is told the work is still pending. Every outcome has a
//! precise status, so an existing readable file is never reported as missing merely
//! because an index row was absent.

use std::path::Path;
use std::path::PathBuf;

use codex_extension_api::FunctionCallError;
use codex_project_intelligence::ContextMapFreshness;
use codex_project_intelligence::EvidenceLineRange;
use codex_project_intelligence::EvidenceReadError;
use codex_project_intelligence::EvidenceReadLocator;
use codex_project_intelligence::EvidenceReadRequest;
use codex_project_intelligence::EvidenceReadResult;
use codex_project_intelligence::EvidenceReader;
use codex_project_intelligence::IndexCancellation;
use codex_project_intelligence::ProjectIndexFileRequest;
use codex_project_intelligence::ProjectIndexer;
use codex_project_intelligence::ProjectIndexerError;
use codex_project_intelligence::ProjectRelativePath;

use crate::index_gate::EXPLICIT_FILE_DEADLINE;
use crate::index_gate::IndexOperation;
use crate::services::ProjectIntelligenceServices;

use super::evidence::read_error;

const READ_DIRECTLY: &str = "Read the file directly instead; an evidence receipt is optional.";

/// One explicit source read.
pub(super) struct ExplicitRead {
    pub(super) project_id: String,
    pub(super) project_roots: Vec<PathBuf>,
    pub(super) requested_root: Option<PathBuf>,
    pub(super) relative_path: ProjectRelativePath,
    pub(super) line_range: Option<EvidenceLineRange>,
    pub(super) max_bytes: u32,
}

/// Reads the file, indexing it first when it has no current entry. Returns the read and
/// whether the file was (re)indexed.
pub(super) async fn read_explicit(
    services: &ProjectIntelligenceServices,
    read: ExplicitRead,
) -> Result<(EvidenceReadResult, bool), FunctionCallError> {
    let unavailable = |error: String| status("indexUnavailable", error);
    let reader = EvidenceReader::new(
        services
            .context_map()
            .await
            .map_err(|error| unavailable(error.to_string()))?
            .clone(),
    );
    let indexer = ProjectIndexer::new(
        services
            .hierarchy()
            .await
            .map_err(|error| unavailable(error.to_string()))?
            .clone(),
        services
            .context_map()
            .await
            .map_err(|error| unavailable(error.to_string()))?
            .clone(),
    );
    let project_id = read.project_id.clone();
    let relative_path = read.relative_path.to_string();
    match services
        .index_gates()
        .run(&project_id, EXPLICIT_FILE_DEADLINE, move |cancellation| {
            read_and_index(reader, indexer, read, cancellation)
        })
        .await
    {
        IndexOperation::Finished(result) => result,
        IndexOperation::Pending => Err(status(
            "timedOutWorkPending",
            format!(
                "reading or indexing {relative_path} is still running and was not abandoned; retry shortly. {READ_DIRECTLY}"
            ),
        )),
        IndexOperation::Failed(error) => Err(unavailable(format!(
            "reading {relative_path} stopped unexpectedly ({error}). {READ_DIRECTLY}"
        ))),
    }
}

async fn read_and_index(
    reader: EvidenceReader,
    indexer: ProjectIndexer,
    read: ExplicitRead,
    cancellation: IndexCancellation,
) -> Result<(EvidenceReadResult, bool), FunctionCallError> {
    let project_root = source_root(
        &read.project_roots,
        read.requested_root.as_ref(),
        &read.relative_path,
    )
    .await?;
    let request = EvidenceReadRequest {
        project_id: read.project_id.clone(),
        project_roots: read.project_roots.clone(),
        locator: EvidenceReadLocator::Source {
            project_root: Some(project_root.clone()),
            relative_path: read.relative_path.clone(),
            line_range: read.line_range,
        },
        max_bytes: read.max_bytes,
    };
    match reader.read(request.clone()).await {
        Ok(result) => return Ok((result, false)),
        Err(
            EvidenceReadError::SourceChanged
            | EvidenceReadError::SourceNotCurrent(
                ContextMapFreshness::Stale | ContextMapFreshness::SourceUnavailable,
            )
            | EvidenceReadError::SourceNotIndexed(_),
        ) => {}
        Err(error) => return Err(read_error(error)),
    }
    let relative_path = read.relative_path;
    match indexer
        .refresh_file_cancellable(
            ProjectIndexFileRequest {
                project_id: read.project_id,
                project_root,
                relative_path: relative_path.clone(),
            },
            cancellation,
        )
        .await
    {
        Ok(_) => {}
        Err(ProjectIndexerError::SourceNotIndexed(_)) => {
            return Err(status(
                "notFound",
                format!("{relative_path} does not exist in the project root"),
            ));
        }
        Err(ProjectIndexerError::InvalidRoot) => {
            return Err(status(
                "outsideRoot",
                format!("{relative_path} is not a regular file inside its project root"),
            ));
        }
        Err(ProjectIndexerError::FileTooLarge(_)) => {
            return Err(status(
                "nonTextOrTooLarge",
                format!(
                    "{relative_path} is too large to index during a read. Inspect the needed part with a bounded command."
                ),
            ));
        }
        Err(error) => {
            return Err(status(
                "indexUnavailable",
                format!("indexing {relative_path} failed ({error}). {READ_DIRECTLY}"),
            ));
        }
    }
    reader
        .read(request)
        .await
        .map(|result| (result, true))
        .map_err(read_error)
}

/// The configured root an explicit path belongs to: the requested root, or the only root
/// in which the path exists. A path that exists under several roots needs `projectRoot`.
async fn source_root(
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
                    "{} is outside the selected project; its roots are: {}",
                    root.display(),
                    list_roots(project_roots)
                ),
            ));
        }
        None => project_roots.to_vec(),
    };
    let path = relative_path.as_str().to_string();
    let lookup = candidates.clone();
    let placements = tokio::task::spawn_blocking(move || {
        lookup
            .into_iter()
            .map(|root| {
                let placement = placement(&root, &path);
                (root, placement)
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|error| status("indexUnavailable", error.to_string()))?;
    let existing = placements
        .iter()
        .filter(|(_, placement)| *placement == Placement::File)
        .map(|(root, _)| root.clone())
        .collect::<Vec<_>>();
    match existing.as_slice() {
        [root] => return Ok(root.clone()),
        [] => {}
        _ => {
            return Err(status(
                "ambiguousRoot",
                format!(
                    "{relative_path} exists in multiple project roots; provide projectRoot as one of: {}",
                    list_roots(&existing)
                ),
            ));
        }
    }
    let described =
        |wanted: Placement| placements.iter().any(|(_, placement)| *placement == wanted);
    if described(Placement::OutsideRoot) {
        return Err(status(
            "outsideRoot",
            format!("{relative_path} resolves outside its project root"),
        ));
    }
    if described(Placement::NotAFile) {
        return Err(status(
            "notAFile",
            format!("{relative_path} is not a regular file"),
        ));
    }
    if described(Placement::Unreadable) {
        return Err(status(
            "indexUnavailable",
            format!("{relative_path} could not be inspected. {READ_DIRECTLY}"),
        ));
    }
    match candidates.as_slice() {
        // A single root is still refreshed, so an index row that outlived its file is
        // marked missing rather than read.
        [root] => Ok(root.clone()),
        _ => Err(status(
            "notFound",
            format!(
                "{relative_path} does not exist under any project root ({})",
                list_roots(&candidates)
            ),
        )),
    }
}

/// Where a relative path lands under one root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Placement {
    File,
    Missing,
    OutsideRoot,
    NotAFile,
    Unreadable,
}

fn placement(root: &Path, relative_path: &str) -> Placement {
    let Ok(root) = std::fs::canonicalize(root) else {
        return Placement::Unreadable;
    };
    let path = match std::fs::canonicalize(root.join(relative_path)) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Placement::Missing;
        }
        Err(_) => return Placement::Unreadable,
    };
    if !path.starts_with(&root) {
        Placement::OutsideRoot
    } else if path.is_file() {
        Placement::File
    } else {
        Placement::NotAFile
    }
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
