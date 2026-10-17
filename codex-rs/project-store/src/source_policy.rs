use std::path::Path;

/// Whether a directory path is within the reserved project-storage namespace.
/// Callers examining a file must pass its parent and separately check resolved
/// aliases on the workspace-owning host. This does not resolve paths, detect
/// hard links, or protect against concurrent same-user filesystem changes.
pub fn is_project_storage_directory(path: &Path) -> bool {
    path.components().any(|component| {
        component.as_os_str().to_str().is_some_and(|name| {
            if cfg!(any(windows, target_os = "macos")) {
                name.eq_ignore_ascii_case(".stateful")
            } else {
                name == ".stateful"
            }
        })
    })
}
