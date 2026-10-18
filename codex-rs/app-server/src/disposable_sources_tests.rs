use super::*;
use crate::disposable_root::tests::assert_refused;
use crate::disposable_root::tests::guard;
use crate::disposable_root::tests::manager_for_loader;
use crate::disposable_root::tests::parent_path;
use anyhow::Context;
use codex_config::CloudConfigBundleLoader;
use codex_core::config::ConfigOverrides;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

fn scratch() -> io::Result<tempfile::TempDir> {
    #[cfg(windows)]
    return tempfile::tempdir_in(std::env::var_os("PUBLIC").ok_or_else(refused)?);
    #[cfg(not(windows))]
    tempfile::tempdir()
}

#[tokio::test]
async fn ancestor_sources_and_git_pointers_refuse_before_policy_or_discovery() -> anyhow::Result<()>
{
    let directory = scratch()?;
    let outer = fs::canonicalize(directory.path())?;
    let root = outer.join("scratch");
    fs::create_dir_all(root.join("home"))?;
    fs::create_dir_all(root.join("project/.git"))?;
    fs::create_dir_all(root.join("project/.codex"))?;
    fs::write(root.join("project/.git/HEAD"), "ref: refs/heads/main\n")?;
    let project = root.join("project");
    let loader = LoaderOverrides::without_managed_config_for_tests();
    let admitted = manager_for_loader(&root, CloudConfigBundleLoader::default(), loader.clone())
        .with_disposable_root(Some(guard(&root)));
    let checked = guard(&root);
    checked
        .before_load(&root.join("home"), &loader, Some(&project))
        .context("canonical project preflight")?;
    let ordinary = AbsolutePathBuf::from_absolute_path(&project)?;
    checked
        .before_load(&root.join("home"), &loader, Some(ordinary.as_path()))
        .context("ordinary project preflight")?;
    let current = admitted
        .load_latest_config(Some(project.clone()))
        .await
        .context("private project config")?;
    let layers = admitted
        .load_config_layers_for_cwd(ordinary)
        .await
        .context("private raw project layers")?;
    let codex_folder = fs::canonicalize(project.join(".codex"))?;
    assert!(layers.all_layers_low_to_high().any(|layer| {
        matches!(&layer.name, ConfigLayerSource::Project { dot_codex_folder }
            if fs::canonicalize(dot_codex_folder.as_path()).ok().as_ref() == Some(&codex_folder))
            && layer.disabled_reason.is_some()
    }));
    // A disabled/untrusted source still produces a read error without the guard.
    fs::create_dir_all(outer.join(".git"))?;
    fs::write(outer.join(".git/HEAD"), "ref: refs/heads/main\n")?;
    fs::create_dir_all(outer.join(".codex/config.toml"))?;
    let unguarded = manager_for_loader(&root, CloudConfigBundleLoader::default(), loader.clone());
    let error = unguarded
        .load_latest_config(Some(root.join("home")))
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Failed to read project config file"),
        "{error}"
    );
    let polled = Arc::new(AtomicBool::new(false));
    let observed = polled.clone();
    let cloud = CloudConfigBundleLoader::new(async move {
        observed.store(true, Ordering::SeqCst);
        Ok(None)
    });
    let guarded = manager_for_loader(&root, cloud, loader).with_disposable_root(Some(guard(&root)));
    for result in [
        guarded.load_latest_config(Some(project.clone())).await,
        guarded
            .load_latest_config_with_session_layers(&current.config_layer_stack, &project)
            .await,
        guarded
            .load_retained_session_config(&current.config_layer_stack, &project)
            .await,
    ] {
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    }
    assert_eq!(polled.load(Ordering::SeqCst), false);
    assert!(guarded.check_thread_model_provider(&current).await.is_err());
    assert!(
        guarded
            .load_config_layers_for_cwd(AbsolutePathBuf::from_absolute_path(&project)?)
            .await
            .is_err()
    );
    assert_eq!(polled.load(Ordering::SeqCst), false);
    fs::remove_dir_all(outer.join(".codex"))?;
    fs::remove_dir_all(outer.join(".git"))?;
    fs::remove_dir_all(project.join(".git"))?;
    fs::write(
        project.join(".git"),
        format!("gitdir: {}\n", outer.display()),
    )?;
    assert!(admitted.load_latest_config(Some(project)).await.is_err());
    Ok(())
}

#[tokio::test]
async fn typed_cwd_and_active_user_paths_are_checked_before_loading() -> anyhow::Result<()> {
    let directory = scratch()?;
    let root = fs::canonicalize(directory.path())?;
    fs::create_dir(root.join("home"))?;
    let outside = scratch()?;
    let mut loader = LoaderOverrides::without_managed_config_for_tests();
    loader.user_config_path = Some(AbsolutePathBuf::from_absolute_path(
        outside.path().join("config.toml"),
    )?);
    let guarded = manager_for_loader(&root, CloudConfigBundleLoader::default(), loader)
        .with_disposable_root(Some(guard(&root)));
    assert_refused(
        guarded.load_latest_config(Some(root.join("home"))).await,
        "active user config outside root",
    );
    let guarded = manager_for_loader(
        &root,
        CloudConfigBundleLoader::default(),
        LoaderOverrides::without_managed_config_for_tests(),
    )
    .with_disposable_root(Some(guard(&root)));
    for cwd in [outside.path().to_path_buf(), parent_path(&root, "home")] {
        assert_refused(
            guarded
                .load_for_cwd(
                    /*request_overrides*/ None,
                    ConfigOverrides {
                        cwd: Some(cwd),
                        ..Default::default()
                    },
                    Some(root.join("home")),
                )
                .await,
            "typed cwd rejection",
        );
    }
    guarded
        .load_for_cwd(
            /*request_overrides*/ None,
            ConfigOverrides {
                cwd: Some(root.join("home")),
                ..Default::default()
            },
            Some(outside.path().to_path_buf()),
        )
        .await?;
    Ok(())
}

#[cfg(windows)]
#[tokio::test]
async fn native_junctions_refuse_at_admission_and_after_manager_creation() -> anyhow::Result<()> {
    let directory = scratch()?;
    let root = fs::canonicalize(directory.path())?;
    fs::create_dir(root.join("home"))?;
    let outside = scratch()?;
    let manager = super::super::tests::manager(&root, CloudConfigBundleLoader::default())
        .with_disposable_root(Some(guard(&root)));
    let current = manager.load_latest_config(Some(root.join("home"))).await?;
    let admitted_guard = guard(&root);
    for target in [outside.path().to_path_buf(), root.join("home")] {
        let alias = root.join("junction");
        create_junction(&alias, &target)?;
        assert!(
            admitted_guard
                .contains(&alias.join("missing/leaf"))
                .is_err()
        );
        assert!(
            DisposableRoot::parse_routes(Some(alias.as_os_str().to_owned()), |_| Some(
                root.join("home").into_os_string()
            ))
            .is_err()
        );
        assert!(
            DisposableRoot::parse_routes(Some(root.as_os_str().to_owned()), |_| Some(
                alias.as_os_str().to_owned()
            ))
            .is_err()
        );
        fs::remove_dir(&alias)?;
    }
    let alias = root.join("home/junction");
    create_junction(&alias, outside.path())?;
    for result in [
        manager.load_latest_config(Some(root.join("home"))).await,
        manager
            .load_latest_config_with_session_layers(&current.config_layer_stack, &current.cwd)
            .await,
        manager
            .load_retained_session_config(&current.config_layer_stack, &current.cwd)
            .await,
    ] {
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    }
    fs::remove_dir(alias)?;
    Ok(())
}

#[cfg(windows)]
fn create_junction(alias: &Path, target: &Path) -> anyhow::Result<()> {
    use std::os::windows::fs::MetadataExt;
    use std::path::Component;
    let parent = fs::canonicalize(alias.parent().context("junction parent")?)?;
    let link = AbsolutePathBuf::from_absolute_path(parent)?
        .join(alias.file_name().context("junction name")?);
    let target = AbsolutePathBuf::from_absolute_path(fs::canonicalize(target)?)?;
    for path in [link.as_path(), target.as_path()] {
        assert!(path.is_absolute());
        assert!(!path.as_os_str().to_string_lossy().contains('/'));
        assert!(
            matches!(path.components().next(), Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), std::path::Prefix::Disk(_)))
        );
    }
    let output = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(link.as_path())
        .arg(target.as_path())
        .output()?;
    assert!(
        output.status.success(),
        "junction creation: {:?}, stdout={}, stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_ne!(fs::symlink_metadata(alias)?.file_attributes() & 0x400, 0);
    assert_eq!(
        fs::canonicalize(alias)?,
        fs::canonicalize(target.as_path())?
    );
    Ok(())
}

#[test]
fn final_source_audit_includes_disabled_layers_but_preserves_system_inputs() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let root = fs::canonicalize(directory.path())?;
    fs::create_dir(root.join("home"))?;
    let outside = tempfile::tempdir()?;
    let path = AbsolutePathBuf::from_absolute_path(outside.path())?;
    let guard = guard(&root);
    for source in [
        ConfigLayerSource::User {
            file: path.clone(),
            profile: None,
        },
        ConfigLayerSource::Project {
            dot_codex_folder: path.clone(),
        },
        ConfigLayerSource::System { file: path },
    ] {
        let system = matches!(source, ConfigLayerSource::System { .. });
        let layers = ConfigLayerStack::new(
            vec![codex_config::ConfigLayerEntry::new_disabled(
                source,
                toml::Value::Table(Default::default()),
                "untrusted",
            )],
            Default::default(),
            Default::default(),
        )?;
        assert_eq!(guard.validate_sources(&layers).is_ok(), system);
    }
    Ok(())
}
