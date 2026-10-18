/// Selects normal startup or an already admitted disposable app-server startup.
///
/// The disposable mode never loads dotenv and must not fall back after helper setup fails.
#[derive(Clone, Copy)]
pub enum Arg0Prelude {
    Normal,
    Disposable,
}

#[cfg(unix)]
pub(crate) fn create_alias(
    executable: &std::path::Path,
    alias: &std::path::Path,
    prelude: Arg0Prelude,
) -> std::io::Result<()> {
    match prelude {
        Arg0Prelude::Normal => std::os::unix::fs::symlink(executable, alias),
        Arg0Prelude::Disposable => std::fs::copy(executable, alias).map(|_| ()),
    }
}

#[cfg(test)]
#[path = "prelude_tests.rs"]
mod tests;
