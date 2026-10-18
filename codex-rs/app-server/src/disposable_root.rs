//! Opt-in child admission for the disposable compaction journey. Never rewrites policy.

use codex_config::types::OAuthCredentialsStoreMode;
use codex_core::config::Config;
use codex_features::Feature;
use codex_login::AuthCredentialsStoreMode;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

#[path = "disposable_sources.rs"]
mod sources;

#[derive(Clone)]
pub(crate) struct DisposableRoot {
    root: PathBuf,
    routes: Vec<PathBuf>,
}

fn refused() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "disposable app-server admission refused",
    )
}

impl DisposableRoot {
    pub(crate) fn prelude() -> io::Result<codex_arg0::Arg0Prelude> {
        let guard = Self::parse_routes(
            std::env::var_os("CODEX_APP_SERVER_DISPOSABLE_ROOT"),
            |key| std::env::var_os(key),
        )?;
        if let Some(root) = guard {
            root.contains(&std::env::current_dir()?)?;
            Ok(codex_arg0::Arg0Prelude::Disposable)
        } else {
            Ok(codex_arg0::Arg0Prelude::Normal)
        }
    }

    pub(crate) fn from_env(strict_config: bool) -> io::Result<Option<Self>> {
        let guard = Self::parse(
            std::env::var_os("CODEX_APP_SERVER_DISPOSABLE_ROOT"),
            strict_config,
            |key| std::env::var_os(key),
        )?;
        if let Some(root) = &guard {
            root.contains(&std::env::current_dir()?)?;
        }
        Ok(guard)
    }

    fn parse(
        root: Option<OsString>,
        strict_config: bool,
        env: impl Fn(&str) -> Option<OsString>,
    ) -> io::Result<Option<Self>> {
        if root.is_some() && !strict_config {
            return Err(refused());
        }
        Self::parse_routes(root, env)
    }

    fn parse_routes(
        root: Option<OsString>,
        env: impl Fn(&str) -> Option<OsString>,
    ) -> io::Result<Option<Self>> {
        let Some(root) = root else {
            return Ok(None);
        };
        let root = PathBuf::from(root);
        check_components(&root)?;
        if !root.is_dir() {
            return Err(refused());
        }
        let mut guard = Self {
            root: fs::canonicalize(root)?,
            routes: Vec::new(),
        };
        for key in [
            "CODEX_HOME",
            "CODEX_SQLITE_HOME",
            "STATEFUL_STORE_ROOT",
            "CODEX_ROLLOUT_TRACE_ROOT",
            "TMP",
            "TEMP",
            "TMPDIR",
        ] {
            let path = PathBuf::from(env(key).ok_or_else(refused)?);
            guard.contains(&path)?;
            if !path.is_dir() {
                return Err(refused());
            }
            guard.routes.push(path);
        }
        for key in ["HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA"] {
            if let Some(path) = env(key) {
                let path = PathBuf::from(path);
                guard.contains(&path)?;
                guard.routes.push(path);
            }
        }
        guard.check_tree()?;
        Ok(Some(guard))
    }

    pub(crate) fn contains(&self, path: &Path) -> io::Result<()> {
        check_components(path)?;
        // Missing leaves are permitted, but their nearest existing ancestor must be private.
        let ancestor = path
            .ancestors()
            .find(|p| fs::symlink_metadata(p).is_ok())
            .ok_or_else(refused)?;
        let canonical = fs::canonicalize(ancestor)?;
        if !canonical.starts_with(&self.root) || (canonical == self.root && ancestor == path) {
            return Err(refused());
        }
        Ok(())
    }

    fn check_tree(&self) -> io::Result<()> {
        // Catch existing auth/config/database/log leaves as well as directory routing.
        // Refuse oversized scratch trees rather than performing an unbounded admission scan.
        let mut pending = vec![self.root.clone()];
        let mut remaining = 10000_usize;
        while let Some(path) = pending.pop() {
            remaining = remaining.checked_sub(1).ok_or_else(refused)?;
            let metadata = fs::symlink_metadata(&path)?;
            check_metadata(&metadata)?;
            if metadata.is_dir() {
                for entry in fs::read_dir(path)? {
                    if pending.len() >= remaining {
                        return Err(refused());
                    }
                    pending.push(entry?.path());
                }
            }
        }
        Ok(())
    }

    pub(crate) fn validate(&self, config: &Config) -> io::Result<()> {
        for path in self.routes.iter().map(PathBuf::as_path).chain([
            config.codex_home.as_path(),
            config.sqlite_config().home(),
            config.log_dir.as_path(),
            config.cwd.as_path(),
        ]) {
            self.contains(path)?;
        }
        for path in &config.workspace_roots {
            self.contains(path.as_path())?;
        }
        self.validate_sources(&config.config_layer_stack)?;
        if config.cli_auth_credentials_store_mode != AuthCredentialsStoreMode::File
            || config.mcp_oauth_credentials_store_mode != OAuthCredentialsStoreMode::File
            || !config.features.enabled(Feature::Sqlite)
            || config.features.enabled(Feature::TokenBudget)
            || config.model_auto_compact_token_limit != Some(100000)
            || config.model_post_turn_compact_threshold_percent != 0
            || config.memories.generate_memories
            || config.memories.use_memories
        {
            return Err(refused());
        }
        self.check_tree()
    }
}

fn check_components(path: &Path) -> io::Result<()> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(refused());
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => check_metadata(&metadata)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn check_metadata(metadata: &fs::Metadata) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(refused());
        }
    }
    if metadata.file_type().is_symlink() {
        return Err(refused());
    }
    Ok(())
}

#[cfg(test)]
#[path = "disposable_root_tests.rs"]
mod tests;
