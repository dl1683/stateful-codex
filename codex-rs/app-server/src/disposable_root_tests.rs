use super::*;
use crate::config_manager::ConfigManager;
use codex_arg0::Arg0DispatchPaths;
use codex_config::CloudConfigBundleLoader;
use codex_config::LoaderOverrides;
use codex_config::test_support::CloudConfigBundleFixture;
use codex_core::config::ConfigOverrides;
use pretty_assertions::assert_eq;
use std::sync::Arc;

fn guard(root: &Path) -> DisposableRoot {
    DisposableRoot::parse(
        Some(root.as_os_str().to_owned()),
        /*strict_config*/ true,
        |_| Some(root.join("home").into_os_string()),
    )
    .unwrap()
    .unwrap()
}

fn manager(root: &Path, cloud: CloudConfigBundleLoader) -> ConfigManager {
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
        LoaderOverrides::without_managed_config_for_tests(),
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
    assert!(guard.contains(&root.join("home/../escape")).is_err());
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
    for requirement in [
        format!("log_dir = {path}"),
        format!("sqlite_home = {path}"),
        "cli_auth_credentials_store = 'keyring'".to_owned(),
    ] {
        let cloud = CloudConfigBundleFixture::loader_with_enterprise_requirement(requirement);
        let guarded = manager(&root, cloud).with_disposable_root(Some(guard(&root)));
        for result in [
            guarded.load_latest_config(Some(root.join("home"))).await,
            guarded
                .load_latest_config_with_session_layers(layers, &current.cwd)
                .await,
            guarded
                .load_retained_session_config(layers, &current.cwd)
                .await,
            guarded.load_default_config().await,
        ] {
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        }
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
