use std::env::JoinPathsError;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::Path;

pub(super) fn prepend_path(
    parent: &Path,
    inherited: Option<&OsStr>,
) -> Result<OsString, JoinPathsError> {
    // Unset contributes no entries; a present empty PATH retains its empty entry.
    std::env::join_paths(
        std::iter::once(parent.to_path_buf())
            .chain(inherited.into_iter().flat_map(std::env::split_paths)),
    )
}

#[cfg(test)]
#[path = "path_env_tests.rs"]
mod tests;
