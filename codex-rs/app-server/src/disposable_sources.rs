use super::DisposableRoot;
use super::check_metadata;
use super::refused;
use codex_config::ConfigLayerSource;
use codex_config::ConfigLayerStack;
use codex_config::LoaderOverrides;
use std::fs;
use std::io;
use std::path::Path;

impl DisposableRoot {
    pub(crate) fn before_load(
        &self,
        home: &Path,
        overrides: &LoaderOverrides,
        cwd: Option<&Path>,
    ) -> io::Result<()> {
        for route in &self.routes {
            self.contains(route)?;
            if !route.is_dir() {
                return Err(refused());
            }
        }
        self.contains(home)?;
        self.contains(&home.join("config.toml"))?;
        self.contains(overrides.user_config_path(home)?.as_path())?;
        self.check_tree()?;
        if let Some(cwd) = cwd {
            self.contains(cwd)?;
            if !overrides.ignore_project_config {
                // This rejects unsafe topology, not selecting a root or reproducing discovery.
                for ancestor in cwd.ancestors() {
                    for name in [".git", ".codex"] {
                        match fs::symlink_metadata(ancestor.join(name)) {
                            Ok(metadata) => {
                                if !fs::canonicalize(ancestor)?.starts_with(&self.root) {
                                    return Err(refused());
                                }
                                check_metadata(&metadata)?;
                                if name == ".git" && !metadata.is_dir() {
                                    return Err(refused());
                                }
                            }
                            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                            Err(error) => return Err(error),
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn validate_sources(&self, layers: &ConfigLayerStack) -> io::Result<()> {
        for layer in layers.all_layers_low_to_high() {
            match &layer.name {
                ConfigLayerSource::User { file, .. } => self.contains(file.as_path())?,
                ConfigLayerSource::Project { dot_codex_folder } => {
                    self.contains(dot_codex_folder.as_path())?;
                    if let Some(hooks) = layer.hooks_config_folder() {
                        self.contains(hooks.as_path())?;
                    }
                }
                ConfigLayerSource::PackagedDefaults { .. }
                | ConfigLayerSource::Mdm { .. }
                | ConfigLayerSource::System { .. }
                | ConfigLayerSource::EnterpriseManaged { .. }
                | ConfigLayerSource::SessionFlags
                | ConfigLayerSource::LegacyManagedConfigTomlFromFile { .. }
                | ConfigLayerSource::LegacyManagedConfigTomlFromMdm => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "disposable_sources_tests.rs"]
mod tests;
