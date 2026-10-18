use super::*;
use crate::config_manager::ConfigManager;
use anyhow::Context;
use codex_arg0::Arg0DispatchPaths;
use codex_config::CloudConfigBundleLoader;
use codex_config::LoaderOverrides;
use codex_config::test_support::CloudConfigBundleFixture;
use codex_core::config::ConfigOverrides;
use pretty_assertions::assert_eq;
use std::sync::Arc;

pub(super) fn parent_path(root: &Path, leaf: &str) -> PathBuf {
    let mut path = root.as_os_str().to_os_string();
    for component in ["home", "..", leaf] {
        path.push(std::path::MAIN_SEPARATOR_STR);
        path.push(component);
    }
    let path = PathBuf::from(path);
    assert!(path.is_absolute());
    assert!(path.components().any(|c| matches!(c, Component::ParentDir)));
    path
}

pub(super) fn assert_refused(result: io::Result<Config>, stage: &str) {
    match result {
        Err(error) => assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{stage}"),
        Ok(config) => panic!(
            "{stage}: expected refusal; managed log/sqlite/auth present: {:?}",
            (
                config.config_layer_stack.requirements().log_dir.is_some(),
                config
                    .config_layer_stack
                    .requirements()
                    .sqlite_home
                    .is_some(),
                config
                    .config_layer_stack
                    .requirements()
                    .cli_auth_credentials_store
                    .is_some(),
            )
        ),
    }
}

pub(super) fn guard(root: &Path) -> DisposableRoot {
    DisposableRoot::parse(
        Some(root.as_os_str().to_owned()),
        /*strict_config*/ true,
        |_| Some(root.join("home").into_os_string()),
    )
    .unwrap()
    .unwrap()
}

pub(super) fn manager(root: &Path, cloud: CloudConfigBundleLoader) -> ConfigManager {
    let mut loader = LoaderOverrides::without_managed_config_for_tests();
    loader.ignore_project_config = true;
    manager_for_loader(root, cloud, loader)
}

pub(super) fn manager_for_loader(
    root: &Path,
    cloud: CloudConfigBundleLoader,
    loader: LoaderOverrides,
) -> ConfigManager {
    let mut overrides = vec![
        ("cli_auth_credentials_store".into(), "file".into()),
        ("mcp_oauth_credentials_store".into(), "file".into()),
        ("features.sqlite".into(), true.into()),
        ("features.token_budget".into(), false.into()),
        ("model_auto_compact_token_limit".into(), 100000_i64.into()),
        (
            "model_post_turn_compact_threshold_percent".into(),
            0_i64.into(),
        ),
        ("memories.generate_memories".into(), false.into()),
        ("memories.use_memories".into(), false.into()),
    ];
    for key in ["sqlite_home", "log_dir"] {
        overrides.push((key.into(), root.join("home").to_str().unwrap().into()));
    }
    ConfigManager::new(
        root.join("home"),
        overrides,
        loader,
        /*strict_config*/ true,
        cloud,
        Arg0DispatchPaths::default(),
        Arc::new(codex_config::NoopThreadConfigLoader),
    )
}

#[test]
fn opt_in_requires_strict_private_existing_routes() -> io::Result<()> {
    assert!(
        DisposableRoot::parse(
            /*root*/ None,
            /*strict_config*/ false,
            |_| panic!("opt out")
        )
        .unwrap()
        .is_none()
    );
    let scratch = tempfile::tempdir()?;
    let root = fs::canonicalize(scratch.path())?;
    fs::create_dir(root.join("home"))?;
    for (path, strict) in [(root.clone(), false), (PathBuf::from("relative"), true)] {
        assert!(
            DisposableRoot::parse(Some(path.into_os_string()), strict, |_| {
                Some(root.join("home").into_os_string())
            })
            .is_err()
        );
    }
    assert!(
        DisposableRoot::parse(
            Some(root.as_os_str().to_owned()),
            /*strict_config*/ true,
            |_| None,
        )
        .is_err()
    );
    let outside = tempfile::tempdir()?;
    assert!(
        DisposableRoot::parse(
            Some(root.as_os_str().to_owned()),
            /*strict_config*/ true,
            |_| Some(outside.path().as_os_str().to_owned()),
        )
        .is_err()
    );
    let guard = guard(&root);
    guard.contains(&root.join("home/new/auth.json"))?;
    assert!(guard.contains(&parent_path(&root, "escape")).is_err());
    assert!(guard.contains(outside.path()).is_err());
    fs::remove_dir(root.join("home"))?;
    assert!(
        DisposableRoot::parse(
            Some(root.into_os_string()),
            /*strict_config*/ true,
            |_| Some(scratch.path().join("home").into_os_string()),
        )
        .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn every_config_return_refuses_managed_escape_and_keeps_policy() -> anyhow::Result<()> {
    let scratch = tempfile::tempdir()?;
    let root = fs::canonicalize(scratch.path())?;
    fs::create_dir(root.join("home"))?;
    let admitted = manager(
        &root,
        CloudConfigBundleFixture::loader_with_enterprise_requirement("[application.network]"),
    )
    .with_disposable_root(Some(guard(&root)));
    let current = admitted.load_latest_config(Some(root.join("home"))).await?;
    assert!(
        current
            .http_client_factory()
            .network_policy()
            .acquire_for_unsupported_sdk()
            .is_err()
    );
    let layers = &current.config_layer_stack;
    // A cloud exact requirement wins over CLI routing, then child admission must refuse it.
    let outside = tempfile::tempdir()?;
    let path = toml::Value::String(outside.path().to_str().unwrap().into());
    let requirement = format!("log_dir = {path}");
    let cloud = CloudConfigBundleFixture::loader_with_enterprise_requirement(requirement);
    let unguarded = manager(&root, cloud);
    let effective = unguarded
        .load_latest_config(Some(root.join("home")))
        .await?;
    assert_eq!(effective.log_dir, outside.path());
    let outside = codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(outside.path())?;
    for (requirement, expected) in [
        (
            format!("log_dir = {path}"),
            ManagedRoute::LogDir(outside.clone()),
        ),
        (
            format!("sqlite_home = {path}"),
            ManagedRoute::SqliteHome(outside),
        ),
        (
            "cli_auth_credentials_store = 'keyring'".to_owned(),
            ManagedRoute::Keyring,
        ),
    ] {
        let mut loader = LoaderOverrides::without_managed_config_for_tests();
        loader.ignore_project_config = true;
        let cloud = match &expected {
            // Cloud layers cannot set authentication policy. Exercise the supported
            // local exact requirement rather than changing that authority boundary.
            ManagedRoute::Keyring => {
                let path = root.join("home/auth-requirements.toml");
                fs::write(&path, &requirement)?;
                loader.system_requirements_path = Some(path);
                CloudConfigBundleLoader::default()
            }
            ManagedRoute::LogDir(_) | ManagedRoute::SqliteHome(_) => {
                let cloud = CloudConfigBundleFixture::loader_with_enterprise_requirement(
                    requirement.clone(),
                );
                let snapshot = cloud.get().await?.context("fixture bundle missing")?;
                assert_eq!(
                    snapshot.requirements_toml.enterprise_managed[0].contents,
                    requirement
                );
                cloud
            }
        };
        let plain = manager_for_loader(&root, cloud.clone(), loader.clone());
        let refreshed = plain
            .load_config_layers_for_cwd(
                codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(root.join("home"))?,
            )
            .await
            .context("unguarded raw requirements")?;
        expected.assert_stack(&refreshed, &format!("{requirement}: refreshed raw stack"));
        let assert_plain = |stage: &str, config: Config| {
            let stage = format!("{requirement}: unguarded {stage}");
            expected.assert_stack(&config.config_layer_stack, &stage);
            match &expected {
                ManagedRoute::LogDir(path) => {
                    assert_eq!(config.log_dir.as_path(), path.as_path(), "{stage}")
                }
                ManagedRoute::SqliteHome(path) => {
                    assert_eq!(config.sqlite_config().home(), path.as_path(), "{stage}")
                }
                ManagedRoute::Keyring => assert_eq!(
                    config.cli_auth_credentials_store_mode,
                    AuthCredentialsStoreMode::Keyring,
                    "{stage}"
                ),
            }
        };
        assert_plain(
            "normal",
            plain
                .load_latest_config(Some(root.join("home")))
                .await
                .context("unguarded normal config")?,
        );
        assert_plain(
            "session",
            plain
                .load_latest_config_with_session_layers(layers, &current.cwd)
                .await
                .context("unguarded session config")?,
        );
        assert_plain(
            "retained",
            plain
                .load_retained_session_config(layers, &current.cwd)
                .await
                .context("unguarded retained config")?,
        );
        let guarded =
            manager_for_loader(&root, cloud, loader).with_disposable_root(Some(guard(&root)));
        assert_refused(
            guarded.load_latest_config(Some(root.join("home"))).await,
            &format!("{requirement}: guarded normal"),
        );
        assert_refused(
            guarded
                .load_latest_config_with_session_layers(layers, &current.cwd)
                .await,
            &format!("{requirement}: guarded session"),
        );
        assert_refused(
            guarded
                .load_retained_session_config(layers, &current.cwd)
                .await,
            &format!("{requirement}: guarded retained"),
        );
        assert_refused(
            guarded.load_default_config().await,
            &format!("{requirement}: guarded default"),
        );
    }
    for rebuilt in [
        admitted
            .load_latest_config_with_session_layers(layers, &current.cwd)
            .await?,
        admitted
            .load_retained_session_config(layers, &current.cwd)
            .await?,
    ] {
        assert_eq!(rebuilt.log_dir, current.log_dir);
        assert_eq!(rebuilt.http_client_factory(), current.http_client_factory());
    }
    let overrides = Some(std::collections::HashMap::from([(
        "features.token_budget".into(),
        serde_json::json!(true),
    )]));
    assert!(
        admitted
            .load_for_cwd(
                overrides,
                ConfigOverrides::default(),
                Some(root.join("home"))
            )
            .await
            .is_err()
    );
    Ok(())
}

enum ManagedRoute {
    LogDir(codex_utils_absolute_path::AbsolutePathBuf),
    SqliteHome(codex_utils_absolute_path::AbsolutePathBuf),
    Keyring,
}

impl ManagedRoute {
    fn assert_stack(&self, stack: &codex_config::ConfigLayerStack, stage: &str) {
        let typed = stack.requirements();
        let raw = stack.requirements_toml();
        match self {
            Self::LogDir(path) => {
                assert_eq!(raw.log_dir.as_ref(), Some(path), "{stage}: raw");
                assert_eq!(
                    typed.log_dir.as_ref().map(|r| &r.value),
                    Some(path),
                    "{stage}: typed"
                );
            }
            Self::SqliteHome(path) => {
                assert_eq!(raw.sqlite_home.as_ref(), Some(path), "{stage}: raw");
                assert_eq!(
                    typed.sqlite_home.as_ref().map(|r| &r.value),
                    Some(path),
                    "{stage}: typed"
                );
            }
            Self::Keyring => {
                assert_eq!(
                    raw.cli_auth_credentials_store,
                    Some(AuthCredentialsStoreMode::Keyring),
                    "{stage}: raw"
                );
                assert_eq!(
                    typed.cli_auth_credentials_store.as_ref().map(|r| &r.value),
                    Some(&AuthCredentialsStoreMode::Keyring),
                    "{stage}: typed"
                );
            }
        }
    }
}

#[tokio::test]
async fn admission_rechecks_credentials_thresholds_memory_and_existing_leaves() -> anyhow::Result<()>
{
    let scratch = tempfile::tempdir()?;
    let root = fs::canonicalize(scratch.path())?;
    fs::create_dir(root.join("home"))?;
    let guard = guard(&root);
    let current = manager(&root, CloudConfigBundleLoader::default())
        .load_latest_config(Some(root.join("home")))
        .await?;
    guard.validate(&current)?;
    for edit in [
        |c: &mut Config| c.cli_auth_credentials_store_mode = AuthCredentialsStoreMode::Keyring,
        |c: &mut Config| c.mcp_oauth_credentials_store_mode = OAuthCredentialsStoreMode::Auto,
        |c: &mut Config| c.model_auto_compact_token_limit = Some(99999),
        |c: &mut Config| c.model_post_turn_compact_threshold_percent = 50,
        |c: &mut Config| c.memories.generate_memories = true,
        |c: &mut Config| c.memories.use_memories = true,
    ] {
        let mut changed = current.clone();
        edit(&mut changed);
        assert_eq!(
            guard.validate(&changed).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied,
        );
    }
    #[cfg(unix)]
    {
        let outside = tempfile::tempdir()?;
        std::os::unix::fs::symlink(outside.path(), root.join("home/auth.json"))?;
        assert!(guard.validate(&current).is_err());
    }
    Ok(())
}
