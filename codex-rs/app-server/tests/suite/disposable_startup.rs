use anyhow::Context;
use anyhow::Result;
use pretty_assertions::assert_eq;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::process::Stdio;
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

enum Startup {
    Normal,
    Disposable,
    DisposableAuthProbe,
}

struct Fixture {
    _directory: TempDir,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self> {
        #[cfg(windows)]
        let directory =
            tempfile::tempdir_in(std::env::var_os("PUBLIC").context("PUBLIC missing")?)?;
        #[cfg(not(windows))]
        let directory = tempfile::tempdir()?;
        let root = directory.path().canonicalize()?;
        for name in ["home", "project", "sqlite", "store", "trace", "temp"] {
            std::fs::create_dir(root.join(name))?;
        }
        std::fs::write(
            root.join("home/config.toml"),
            r#"
cli_auth_credentials_store = "file"
mcp_oauth_credentials_store = "file"
model_auto_compact_token_limit = 100000
model_post_turn_compact_threshold_percent = 0
[features]
sqlite = true
token_budget = false
[memories]
generate_memories = false
use_memories = false
"#,
        )?;
        Ok(Self {
            _directory: directory,
            root,
        })
    }

    async fn run(&self, startup: Startup, args: &[&str]) -> Result<(ExitStatus, String)> {
        let mut command = Command::new(codex_utils_cargo_bin::cargo_bin("codex-app-server")?);
        command.env_clear();
        for key in [
            "PATH",
            "SYSTEMROOT",
            "WINDIR",
            "COMSPEC",
            "PATHEXT",
            "PROGRAMDATA",
            "PUBLIC",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        for (key, name) in [
            ("CODEX_HOME", "home"),
            ("CODEX_SQLITE_HOME", "sqlite"),
            ("STATEFUL_STORE_ROOT", "store"),
            ("CODEX_ROLLOUT_TRACE_ROOT", "trace"),
            ("TMP", "temp"),
            ("TEMP", "temp"),
            ("TMPDIR", "temp"),
            ("HOME", "home"),
            ("USERPROFILE", "home"),
            ("APPDATA", "home"),
            ("LOCALAPPDATA", "home"),
        ] {
            command.env(key, self.root.join(name));
        }
        command.env(
            "CODEX_APP_SERVER_MANAGED_CONFIG_PATH",
            self.root.join("home/managed_config.toml"),
        );
        if matches!(startup, Startup::Disposable | Startup::DisposableAuthProbe) {
            command.env("CODEX_APP_SERVER_DISPOSABLE_ROOT", &self.root);
        }
        if matches!(startup, Startup::DisposableAuthProbe) {
            command.env("OPENAI_FEDERATION_RULE_ID", "synthetic-fixture");
        }
        let mut child = command
            .current_dir(self.root.join("project"))
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let stdout = child.stdout.take().context("stdout")?;
        let stderr = child.stderr.take().context("stderr")?;
        let result = tokio::time::timeout(Duration::from_secs(/*secs*/ 20), async {
            tokio::try_join!(child.wait(), capture(stdout), capture(stderr))
        })
        .await;
        if !matches!(result, Ok(Ok(_))) {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        let (status, stdout, stderr) = result.context("startup deadline")??;
        let mut text = String::from_utf8(stdout)?;
        text.push_str(&String::from_utf8(stderr)?);
        Ok((status, text))
    }
}

async fn capture(pipe: impl AsyncRead + Unpin) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    pipe.take(/*limit*/ 65537).read_to_end(&mut bytes).await?;
    if bytes.len() > 65536 {
        return Err(std::io::Error::other("startup output exceeded 64 KiB"));
    }
    Ok(bytes)
}

#[tokio::test]
async fn routes_and_strict_mode_refuse_before_config_but_helpers_still_dispatch() -> Result<()> {
    let fixture = Fixture::new()?;
    std::fs::write(
        fixture.root.join("home/config.toml"),
        "distinct_tripwire = [",
    )?;
    let (status, text) = fixture
        .run(Startup::Disposable, &["--listen", "stdio://"])
        .await?;
    assert!(!status.success());
    assert!(
        text.contains("disposable app-server admission refused"),
        "{text}"
    );
    let (status, text) = fixture
        .run(
            Startup::Disposable,
            &["--strict-config", "--listen", "stdio://"],
        )
        .await?;
    assert!(!status.success());
    assert!(
        !text.contains("disposable app-server admission refused"),
        "{text}"
    );
    assert!(text.contains("config.toml"), "{text}");
    std::fs::remove_dir(fixture.root.join("store"))?;
    let (status, text) = fixture
        .run(
            Startup::Disposable,
            &["--strict-config", "--listen", "stdio://"],
        )
        .await?;
    assert!(!status.success());
    assert!(
        text.contains("disposable app-server admission refused"),
        "{text}"
    );
    assert!(!fixture.root.join("sqlite/state_5.sqlite").exists());
    let (status, text) = fixture
        .run(
            Startup::Disposable,
            &["--codex-run-as-apply-patch", "not a patch"],
        )
        .await?;
    assert!(!status.success());
    assert!(
        !text.contains("disposable app-server admission refused"),
        "{text}"
    );
    assert!(text.to_lowercase().contains("patch"), "{text}");
    Ok(())
}

#[tokio::test]
async fn disposable_dotenv_is_ignored_while_normal_startup_keeps_it() -> Result<()> {
    let fixture = Fixture::new()?;
    std::fs::write(
        fixture.root.join("home/.env"),
        "OPENAI_FEDERATION_RULE_ID=synthetic-fixture\n",
    )?;
    let (status, text) = fixture
        .run(
            Startup::Normal,
            &["--strict-config", "--listen", "stdio://"],
        )
        .await?;
    assert!(!status.success());
    assert!(
        text.contains("workload identity requires OPENAI_IDENTITY_TOKEN_FILE"),
        "{text}"
    );
    let (status, text) = fixture
        .run(
            Startup::Disposable,
            &["--strict-config", "--listen", "stdio://"],
        )
        .await?;
    assert!(status.success(), "{text}");
    assert!(!text.contains("workload identity"), "{text}");
    assert!(fixture.root.join("sqlite/state_5.sqlite").is_file());
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("home/.env"))?,
        "OPENAI_FEDERATION_RULE_ID=synthetic-fixture\n"
    );
    Ok(())
}

#[tokio::test]
async fn rejected_effective_routes_precede_auth_and_sqlite() -> Result<()> {
    let fixture = Fixture::new()?;
    let outside = Fixture::new()?;
    let quoted = toml::Value::String(outside.root.join("sqlite").display().to_string());
    for setting in [
        "cli_auth_credentials_store=\"keyring\"".to_owned(),
        format!("log_dir={quoted}"),
        format!("sqlite_home={quoted}"),
    ] {
        let (status, text) = fixture
            .run(
                Startup::DisposableAuthProbe,
                &["--strict-config", "--listen", "stdio://", "-c", &setting],
            )
            .await?;
        assert!(!status.success());
        assert!(
            text.contains("disposable app-server admission refused"),
            "{text}"
        );
        assert!(!text.contains("workload identity requires"), "{text}");
        assert!(!fixture.root.join("sqlite/state_5.sqlite").exists());
        assert_eq!(std::fs::read_dir(outside.root.join("sqlite"))?.count(), 0);
    }
    let (status, text) = fixture
        .run(
            Startup::DisposableAuthProbe,
            &["--strict-config", "--listen", "stdio://"],
        )
        .await?;
    assert!(!status.success());
    assert!(
        text.contains("workload identity requires OPENAI_IDENTITY_TOKEN_FILE"),
        "{text}"
    );
    assert!(!fixture.root.join("sqlite/state_5.sqlite").exists());
    Ok(())
}

#[cfg(windows)]
#[tokio::test]
async fn junction_home_is_refused_before_dotenv_or_helper_writes() -> Result<()> {
    use codex_utils_absolute_path::AbsolutePathBuf;
    use std::os::windows::fs::MetadataExt;
    let fixture = Fixture::new()?;
    let outside = Fixture::new()?;
    std::fs::write(
        outside.root.join("home/.env"),
        "OPENAI_FEDERATION_RULE_ID=synthetic-fixture\n",
    )?;
    std::fs::rename(
        fixture.root.join("home"),
        fixture.root.join("original-home"),
    )?;
    let home = AbsolutePathBuf::from_absolute_path(&fixture.root)?.join("home");
    let target = AbsolutePathBuf::from_absolute_path(outside.root.join("home"))?;
    for path in [home.as_path(), target.as_path()] {
        assert!(path.is_absolute());
        assert!(!path.as_os_str().to_string_lossy().contains('/'));
        assert!(
            matches!(path.components().next(), Some(std::path::Component::Prefix(prefix))
            if matches!(prefix.kind(), std::path::Prefix::Disk(_)))
        );
    }
    let output = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(home.as_path())
        .arg(target.as_path())
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_ne!(
        std::fs::symlink_metadata(home.as_path())?.file_attributes() & 0x400,
        0
    );
    assert_eq!(
        std::fs::canonicalize(home.as_path())?,
        std::fs::canonicalize(target.as_path())?
    );
    let (status, text) = fixture
        .run(
            Startup::Disposable,
            &["--strict-config", "--listen", "stdio://"],
        )
        .await?;
    assert!(!status.success());
    assert!(
        text.contains("disposable app-server admission refused"),
        "{text}"
    );
    assert!(!text.contains("workload identity requires"), "{text}");
    assert!(!outside.root.join("home/tmp").exists());
    assert_eq!(std::fs::read_dir(outside.root.join("sqlite"))?.count(), 0);
    std::fs::remove_dir(fixture.root.join("home"))?;
    Ok(())
}
