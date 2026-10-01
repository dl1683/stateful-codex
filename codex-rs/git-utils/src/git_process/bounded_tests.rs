use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::process::Command;
use tokio::time::Instant;

use super::*;

const GENEROUS: Duration = Duration::from_secs(30);
const SHORT: Duration = Duration::from_millis(300);

/// A descendant that signals readiness, waits for release, then records survival.
#[cfg(unix)]
const DESCENDANT: &str = r#": > "$READY_FILE"; while [ ! -f "$RELEASE_FILE" ]; do sleep 0.01; done; sleep 1; : > "$SURVIVED_FILE""#;
#[cfg(windows)]
const DESCENDANT: &str = "Set-Content -LiteralPath $env:READY_FILE -Value ready; while (-not (Test-Path $env:RELEASE_FILE)) { Start-Sleep -Milliseconds 25 }; Start-Sleep -Seconds 1; Set-Content -LiteralPath $env:SURVIVED_FILE -Value survived";

/// How the fixture's parent process treats its readiness-signalling descendant.
#[derive(Clone, Copy)]
enum Parent {
    /// Waits for the descendant, keeping both alive.
    Waits,
    /// Exits immediately while the descendant keeps the output pipes open.
    Exits,
    /// Writes 64 KiB to stdout, then waits for the descendant.
    FloodsStdoutThenWaits,
}

#[cfg(unix)]
fn parent_script(parent: Parent) -> String {
    match parent {
        Parent::Waits => format!("( {DESCENDANT} ) & wait"),
        Parent::Exits => format!("( {DESCENDANT} ) &"),
        Parent::FloodsStdoutThenWaits => {
            format!("( {DESCENDANT} ) & head -c 65536 /dev/zero; wait")
        }
    }
}

#[cfg(windows)]
fn parent_script(parent: Parent) -> String {
    let start = format!(
        "$child = Start-Process -FilePath powershell.exe -ArgumentList @('-NoProfile', '-NonInteractive', '-Command', '{DESCENDANT}') -PassThru -NoNewWindow"
    );
    match parent {
        Parent::Waits => format!("{start}; Wait-Process -Id $child.Id"),
        Parent::Exits => start,
        Parent::FloodsStdoutThenWaits => {
            format!("{start}; [Console]::Out.Write('x' * 65536); Wait-Process -Id $child.Id")
        }
    }
}

fn shell(script: &str) -> Command {
    #[cfg(unix)]
    let command = {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    };
    #[cfg(windows)]
    let command = {
        let mut command = Command::new("powershell.exe");
        command
            .args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg(script);
        command
    };
    command
}

struct Fixture {
    _temp_dir: TempDir,
    ready: PathBuf,
    release: PathBuf,
    survived: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        Self {
            ready: temp_dir.path().join("ready"),
            release: temp_dir.path().join("release"),
            survived: temp_dir.path().join("survived"),
            _temp_dir: temp_dir,
        }
    }

    fn command(&self, parent: Parent) -> Command {
        let mut command = shell(&parent_script(parent));
        command
            .env("READY_FILE", &self.ready)
            .env("RELEASE_FILE", &self.release)
            .env("SURVIVED_FILE", &self.survived);
        command
    }

    /// Releases the descendant and checks that cleanup already terminated it.
    async fn assert_descendant_terminated(&self) {
        std::fs::write(&self.release, "release").expect("release descendant");
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(
            !self.survived.exists(),
            "descendant survived Git process cleanup"
        );
    }
}

async fn wait_for_file(path: &Path) {
    tokio::time::timeout(GENEROUS, async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("wait for fixture readiness");
}

/// Spawns under required containment and waits until the descendant is running,
/// so slow shell startup never counts against the short deadline under test.
async fn spawn_ready(
    fixture: &Fixture,
    parent: Parent,
) -> (tokio::process::Child, KillGitProcessTreeOnDrop) {
    let (child, process_tree) = spawn_git_command(
        &mut fixture.command(parent),
        GitProcessContainment::Required,
    )
    .expect("spawn fixture");
    wait_for_file(&fixture.ready).await;
    (child, process_tree)
}

fn budget(deadline_after: Duration, output_allowance: usize) -> GitCommandBudget {
    GitCommandBudget::new(Instant::now() + deadline_after, output_allowance)
}

#[tokio::test]
async fn deadline_after_readiness_times_out_and_terminates_descendants() {
    let fixture = Fixture::new();
    let (child, process_tree) = spawn_ready(&fixture, Parent::Waits).await;

    let result = collect_git_output(
        child,
        process_tree,
        &budget(SHORT, /*output_allowance*/ 1024),
        GitCommandOutputCap::SharedAllowance,
    )
    .await;

    assert!(
        matches!(result, Err(GitCommandError::Timeout)),
        "{result:?}"
    );
    fixture.assert_descendant_terminated().await;
}

#[tokio::test]
async fn descendant_holding_pipes_after_parent_exit_times_out() {
    let fixture = Fixture::new();
    let (mut child, process_tree) = spawn_ready(&fixture, Parent::Exits).await;
    tokio::time::timeout(GENEROUS, child.wait())
        .await
        .expect("parent exits")
        .expect("wait for parent");

    let result = collect_git_output(
        child,
        process_tree,
        &budget(SHORT, /*output_allowance*/ 1024),
        GitCommandOutputCap::SharedAllowance,
    )
    .await;

    assert!(
        matches!(result, Err(GitCommandError::Timeout)),
        "{result:?}"
    );
    fixture.assert_descendant_terminated().await;
}

#[tokio::test]
async fn output_limit_fails_and_terminates_descendants() {
    let fixture = Fixture::new();
    let (child, process_tree) = spawn_ready(&fixture, Parent::FloodsStdoutThenWaits).await;
    let budget = budget(GENEROUS, /*output_allowance*/ 1024);

    let result = collect_git_output(
        child,
        process_tree,
        &budget,
        GitCommandOutputCap::SharedAllowance,
    )
    .await;

    assert!(
        matches!(result, Err(GitCommandError::OutputLimit)),
        "{result:?}"
    );
    assert_eq!(budget.remaining_output(), 0);
    fixture.assert_descendant_terminated().await;
}

#[tokio::test]
async fn caller_cancellation_terminates_descendants() {
    let fixture = Fixture::new();
    let budget = budget(GENEROUS, /*output_allowance*/ 1024);
    let mut command = fixture.command(Parent::Waits);

    tokio::select! {
        result = run_git_command_with_budget(
            &mut command,
            &budget,
            GitCommandOutputCap::SharedAllowance,
        ) => panic!("fixture finished before cancellation: {result:?}"),
        () = wait_for_file(&fixture.ready) => {}
    }

    fixture.assert_descendant_terminated().await;
}

#[tokio::test]
async fn stderr_beyond_pipe_capacity_before_stdout_is_drained_concurrently() {
    const STDERR_BYTES: usize = 256 * 1024;
    #[cfg(unix)]
    let script = format!("head -c {STDERR_BYTES} /dev/zero | tr '\\0' x >&2; printf done");
    #[cfg(windows)]
    let script =
        format!("[Console]::Error.Write('x' * {STDERR_BYTES}); [Console]::Out.Write('done')");
    let budget = budget(GENEROUS, /*output_allowance*/ 1024 * 1024);

    let output = run_git_command_with_budget(
        &mut shell(&script),
        &budget,
        GitCommandOutputCap::SharedAllowance,
    )
    .await
    .expect("run fixture");

    assert_eq!(
        (output.status.success(), output.stdout, output.stderr),
        (true, b"done".to_vec(), vec![b'x'; STDERR_BYTES])
    );
    assert_eq!(budget.remaining_output(), 1024 * 1024 - STDERR_BYTES - 4);
}

#[tokio::test]
async fn metadata_cap_applies_below_the_shared_allowance() {
    #[cfg(unix)]
    let script = "head -c 65537 /dev/zero";
    #[cfg(windows)]
    let script = "[Console]::Out.Write('x' * 65537)";

    let result = run_git_command_with_budget(
        &mut shell(script),
        &budget(GENEROUS, /*output_allowance*/ 1024 * 1024),
        GitCommandOutputCap::Metadata,
    )
    .await;

    assert!(
        matches!(result, Err(GitCommandError::OutputLimit)),
        "{result:?}"
    );
}

#[tokio::test]
async fn exhausted_allowance_still_accepts_exact_output_at_eof() {
    #[cfg(unix)]
    let script = "printf done";
    #[cfg(windows)]
    let script = "[Console]::Out.Write('done')";
    let budget = budget(GENEROUS, /*output_allowance*/ 4);

    let output =
        run_git_command_with_budget(&mut shell(script), &budget, GitCommandOutputCap::Metadata)
            .await
            .expect("run fixture");

    assert_eq!(
        (output.stdout, output.stderr, budget.remaining_output()),
        (b"done".to_vec(), Vec::new(), 0)
    );
}

#[tokio::test]
async fn spawn_errors_are_preserved_and_expired_budgets_do_not_spawn() {
    let missing = || Command::new("codex-git-utils-missing-program");
    let spawn = run_git_command_with_budget(
        &mut missing(),
        &budget(GENEROUS, /*output_allowance*/ 1024),
        GitCommandOutputCap::Metadata,
    )
    .await;
    let expired = run_git_command_with_budget(
        &mut missing(),
        &GitCommandBudget::new(Instant::now(), /*output_allowance*/ 1024),
        GitCommandOutputCap::Metadata,
    )
    .await;

    assert!(
        matches!(
            &spawn,
            Err(GitCommandError::Spawn(GitSpawnError::Spawn(error)))
                if error.kind() == std::io::ErrorKind::NotFound
        ),
        "{spawn:?}"
    );
    assert!(
        matches!(expired, Err(GitCommandError::Timeout)),
        "{expired:?}"
    );
}
