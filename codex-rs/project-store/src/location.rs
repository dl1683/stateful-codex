use std::fs;
use std::io;
use std::path::Path;

use codex_utils_absolute_path::AbsolutePathBuf;
use thiserror::Error;

/// A selected project and its single canonical database location.
/// Paths are canonicalized on the workspace-owning host, never in an engine home.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectStoreLocation {
    project_root: AbsolutePathBuf,
    database_path: AbsolutePathBuf,
}

impl ProjectStoreLocation {
    /// Explicit selection wins, including for non-Git or nested projects.
    /// The directory must exist; the store itself need not exist yet.
    pub fn select_project(project_root: &AbsolutePathBuf) -> Result<Self, ResolveError> {
        let project_root = canonical_directory(project_root)?;
        Self::from_root(project_root)
    }

    /// Discover existing stores only inside the declared workspace boundary.
    /// Without a store, select the nearest Git marker inside that boundary.
    /// A non-Git directory or nested stores require explicit selection.
    pub fn resolve_workspace(
        cwd: &AbsolutePathBuf,
        workspace_boundary: &AbsolutePathBuf,
    ) -> Result<Self, ResolveError> {
        let cwd = canonical_directory(cwd)?;
        let boundary = canonical_directory(workspace_boundary)?;
        if !cwd.as_path().starts_with(boundary.as_path()) {
            return Err(ResolveError::OutsideWorkspace);
        }

        let mut existing = None;
        let mut git_root = None;
        for ancestor in cwd.as_path().ancestors() {
            let root = AbsolutePathBuf::from_absolute_path(ancestor)?;
            let location = Self::from_root(root.clone())?;
            if location.database_path.as_path().try_exists()? {
                if existing.is_some() {
                    return Err(ResolveError::AmbiguousProjects);
                }
                existing = Some(location);
            }
            if git_root.is_none() {
                match fs::symlink_metadata(ancestor.join(".git")) {
                    Ok(marker) if marker.is_dir() || marker.is_file() => git_root = Some(root),
                    Ok(_) => return Err(ResolveError::InvalidGitMarker),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
            if ancestor == boundary.as_path() {
                break;
            }
        }
        if let Some(location) = existing {
            return Ok(location);
        }
        let root = git_root.ok_or(ResolveError::ExplicitProjectRequired)?;
        Self::from_root(root)
    }

    pub fn project_root(&self) -> &AbsolutePathBuf {
        &self.project_root
    }

    pub fn database_path(&self) -> &AbsolutePathBuf {
        &self.database_path
    }

    fn from_root(project_root: AbsolutePathBuf) -> Result<Self, ResolveError> {
        let state_dir = project_root.join(".stateful");
        validate_component(state_dir.as_path(), ComponentKind::Directory)?;
        let database_path = state_dir.join("memory.sqlite");
        validate_component(database_path.as_path(), ComponentKind::File)?;
        Ok(Self {
            project_root,
            database_path,
        })
    }
}

#[derive(Clone, Copy)]
enum ComponentKind {
    Directory,
    File,
}

fn canonical_directory(path: &AbsolutePathBuf) -> Result<AbsolutePathBuf, ResolveError> {
    let canonical = path.canonicalize()?;
    if !fs::metadata(&canonical)?.is_dir() {
        return Err(ResolveError::NotDirectory);
    }
    Ok(canonical)
}

fn validate_component(path: &Path, kind: ComponentKind) -> Result<(), ResolveError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Junctions and other reparse points can redirect storage too.
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(ResolveError::UnsafeStorePath);
        }
    }
    if metadata.file_type().is_symlink()
        || match kind {
            ComponentKind::Directory => !metadata.is_dir(),
            ComponentKind::File => !metadata.is_file(),
        }
    {
        return Err(ResolveError::UnsafeStorePath);
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum ResolveError {
    #[error("project-store path could not be inspected: {0}")]
    Io(#[from] io::Error),
    #[error("the selected project must be an existing directory")]
    NotDirectory,
    #[error("the working directory is outside the declared workspace boundary")]
    OutsideWorkspace,
    #[error(
        "select a non-Git project directory explicitly; no in-boundary store or Git root exists"
    )]
    ExplicitProjectRequired,
    #[error("nested project stores require an explicit project selection")]
    AmbiguousProjects,
    #[error(".stateful must be a directory and memory.sqlite a regular file, without links")]
    UnsafeStorePath,
    #[error("the Git marker must be a directory or regular file")]
    InvalidGitMarker,
}

#[cfg(test)]
#[path = "location_tests.rs"]
mod tests;
