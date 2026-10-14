//! The first local project root owns every stored check directory.

use std::path::Path;
use std::path::PathBuf;

use crate::acceptance::validate_path;

pub(crate) struct StoredCwd {
    pub(crate) text: String,
    pub(crate) directory: PathBuf,
}

#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum CwdReason {
    #[error("checkCwd needs a local project root")]
    NeedsRoot,
    #[error("checkCwd must be an existing directory inside the first project root")]
    InvalidDirectory,
    #[error("checkCwd must be inside the first project root")]
    OutsideRoot,
}

pub(crate) fn stored_check_directory(
    project_root: Option<&Path>,
    input: &str,
) -> Result<StoredCwd, CwdReason> {
    let root = project_root.ok_or(CwdReason::NeedsRoot)?;
    validate_path(input).map_err(|_| CwdReason::InvalidDirectory)?;
    let root = root
        .canonicalize()
        .map_err(|_| CwdReason::InvalidDirectory)?;
    let directory = root
        .join(input)
        .canonicalize()
        .map_err(|_| CwdReason::InvalidDirectory)?;
    if !directory.is_dir() {
        return Err(CwdReason::InvalidDirectory);
    }
    let relative = directory
        .strip_prefix(&root)
        .map_err(|_| CwdReason::OutsideRoot)?;
    let parts = relative
        .iter()
        .map(|part| part.to_str().ok_or(CwdReason::InvalidDirectory))
        .collect::<Result<Vec<_>, _>>()?;
    let text = if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    };
    validate_path(&text).map_err(|_| CwdReason::InvalidDirectory)?;
    Ok(StoredCwd { text, directory })
}

#[cfg(test)]
#[path = "acceptance_check_dir_tests.rs"]
mod tests;
