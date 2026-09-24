import json
import shutil
import sqlite3
import subprocess
import time
import uuid
from contextlib import closing
from pathlib import Path

from protocol import notebook_summary, sha256_file

PROVIDER_FAILURE_MARKERS = (
    "401 unauthorized",
    "authentication failed",
    "failed to refresh token",
    "invalid authentication",
    "rate limit exceeded",
    "too many requests",
    "internal server error",
    "service unavailable",
    "bad gateway",
    "gateway timeout",
)

CONTAINER_INFRASTRUCTURE_FAILURE_MARKERS = (
    "error waiting for container: unexpected eof",
    "cannot connect to the docker daemon",
    "the docker daemon is not running",
)


def assess_operational_validity(
    result: dict[str, object], *, stateful: bool
) -> dict[str, object]:
    notebook = result.get("notebook")
    notebook_replay = result.get("notebookReplay")
    integrity_audit = result.get("integrityAudit")
    control = result.get("control")
    stateful_state = result.get("statefulState")
    checks = {
        "agentCompleted": result.get("agentStarted") is True
        and result.get("timedOut") is False
        and result.get("exitCode") == 0,
        "structuredAnswer": isinstance(result.get("answer"), str),
        "submittedNotebook": isinstance(notebook, dict)
        and notebook.get("status") == "valid",
        "offlineNotebookReplay": isinstance(notebook_replay, dict)
        and notebook_replay.get("status") == "reproducible",
        "protocolAudit": isinstance(integrity_audit, dict)
        and integrity_audit.get("passed") is True,
        "artifactHashes": all(
            isinstance(result.get(key), str) and bool(result[key])
            for key in ("imageId", "bundleSha256", "workspaceManifestSha256")
        )
        and isinstance(control, dict)
        and bool(control),
        "terminalStatefulRun": isinstance(stateful_state, dict)
        and stateful_state.get("valid") is True
        if stateful
        else True,
    }
    failures = [name for name, passed in checks.items() if not passed]
    return {
        "valid": not failures,
        "checks": checks,
        "failures": failures,
    }


def classify_provider_failure(*paths: Path) -> str | None:
    text = "\n".join(
        path.read_text(encoding="utf-8", errors="replace")[-2_000_000:]
        for path in paths
        if path.is_file()
    ).lower()
    marker = next(
        (marker for marker in PROVIDER_FAILURE_MARKERS if marker in text), None
    )
    return f"provider or authentication failure: {marker}" if marker else None


def classify_container_infrastructure_failure(*paths: Path) -> str | None:
    text = "\n".join(
        path.read_text(encoding="utf-8", errors="replace")[-2_000_000:]
        for path in paths
        if path.is_file()
    ).lower()
    marker = next(
        (
            marker
            for marker in CONTAINER_INFRASTRUCTURE_FAILURE_MARKERS
            if marker in text
        ),
        None,
    )
    return f"container infrastructure failure: {marker}" if marker else None


def inspect_stateful_state(state_dir: Path) -> dict[str, object]:
    try:
        return _inspect_stateful_state(state_dir)
    except (OSError, sqlite3.Error) as error:
        return {
            "status": "inspection_error",
            "valid": False,
            "reason": str(error),
        }


def _inspect_stateful_state(state_dir: Path) -> dict[str, object]:
    runtime_path = state_dir / "stateful_runtime_1.sqlite"
    intelligence_path = state_dir / "project_intelligence_1.sqlite"
    paths = {
        "runtime": runtime_path,
        "projectIntelligence": intelligence_path,
    }
    database_status: dict[str, object] = {}
    for name, path in paths.items():
        if not path.is_file():
            database_status[name] = {"exists": False, "integrity": None}
            continue
        with closing(
            sqlite3.connect(f"file:{path.as_posix()}?mode=ro", uri=True)
        ) as database:
            integrity = [row[0] for row in database.execute("PRAGMA integrity_check")]
        files = {
            candidate.name: sha256_file(candidate)
            for candidate in (
                path,
                path.with_name(f"{path.name}-wal"),
                path.with_name(f"{path.name}-shm"),
            )
            if candidate.is_file()
        }
        database_status[name] = {
            "exists": True,
            "integrity": integrity,
            "files": files,
        }

    if not runtime_path.is_file() or not intelligence_path.is_file():
        return {
            "status": "missing",
            "valid": False,
            "databases": database_status,
        }

    with closing(
        sqlite3.connect(f"file:{runtime_path.as_posix()}?mode=ro", uri=True)
    ) as runtime:
        runtime.row_factory = sqlite3.Row
        run = runtime.execute(
            """SELECT id, project_id, mode, status, revision, strategy_revision,
                      result, continuations_used
                 FROM stateful_runs
             ORDER BY updated_at_ms DESC, id LIMIT 1"""
        ).fetchone()
        run_count = runtime.execute("SELECT COUNT(*) FROM stateful_runs").fetchone()[0]
        obligation_count = runtime.execute(
            "SELECT COUNT(*) FROM stateful_obligations"
        ).fetchone()[0]
    if run is None:
        return {
            "status": "missing_run",
            "valid": False,
            "databases": database_status,
            "counts": {"runs": run_count, "obligations": obligation_count},
        }

    with closing(
        sqlite3.connect(f"file:{intelligence_path.as_posix()}?mode=ro", uri=True)
    ) as intelligence:
        project_id = run["project_id"]
        counts = {
            "runs": run_count,
            "obligations": obligation_count,
            "hierarchyNodes": intelligence.execute(
                "SELECT COUNT(*) FROM hierarchy_nodes WHERE project_id = ?",
                (project_id,),
            ).fetchone()[0],
            "contextMapEntries": intelligence.execute(
                "SELECT COUNT(*) FROM context_map_entries WHERE project_id = ?",
                (project_id,),
            ).fetchone()[0],
            "blackboardEntries": intelligence.execute(
                "SELECT COUNT(*) FROM blackboard_entries WHERE project_id = ?",
                (project_id,),
            ).fetchone()[0],
            "blackboardRelations": intelligence.execute(
                "SELECT COUNT(*) FROM blackboard_relations WHERE project_id = ?",
                (project_id,),
            ).fetchone()[0],
        }

    integrity_ok = all(
        value.get("integrity") == ["ok"]
        for value in database_status.values()
        if isinstance(value, dict)
    )
    run_record = dict(run)
    return {
        "status": "valid"
        if integrity_ok and run["status"] == "completed"
        else "invalid",
        "valid": integrity_ok and run["status"] == "completed",
        "databases": database_status,
        "run": run_record,
        "counts": counts,
    }


def reexecute_notebook(
    input_workspace: Path,
    notebook: Path,
    task_root: Path,
    image: str,
    timeout_seconds: int,
) -> dict[str, object]:
    if not notebook.is_file():
        return {"status": "skipped", "reason": "notebook.ipynb is missing"}

    replay = task_root / "notebook-replay"
    shutil.copytree(input_workspace, replay)
    shutil.copy2(notebook, replay / "notebook.ipynb")
    container_name = f"stateful-bixbench-replay-{uuid.uuid4().hex[:12]}"
    output = replay / "notebook.reexecuted.ipynb"
    command = [
        "docker",
        "run",
        "--rm",
        "--name",
        container_name,
        "--network",
        "none",
        "--mount",
        f"type=bind,source={replay.resolve()},target=/workspace",
        image,
        "jupyter",
        "nbconvert",
        "--to",
        "notebook",
        "--execute",
        "/workspace/notebook.ipynb",
        "--output",
        output.name,
        "--output-dir",
        "/workspace",
        f"--ExecutePreprocessor.timeout={timeout_seconds}",
    ]
    started = time.monotonic()
    timed_out = False
    logs = task_root / "logs"
    with (
        (logs / "notebook-replay.stdout").open("wb") as stdout,
        (logs / "notebook-replay.stderr").open("wb") as stderr,
    ):
        try:
            completed = subprocess.run(
                command,
                timeout=timeout_seconds + 60,
                check=False,
                stdout=stdout,
                stderr=stderr,
            )
            exit_code = completed.returncode
        except subprocess.TimeoutExpired:
            timed_out = True
            exit_code = None
            subprocess.run(
                ["docker", "rm", "--force", container_name],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=False,
            )
    result: dict[str, object] = {
        "status": "timeout" if timed_out else "failed",
        "network": "none",
        "exitCode": exit_code,
        "durationSeconds": round(time.monotonic() - started, 3),
    }
    if not timed_out and exit_code == 0 and output.is_file():
        try:
            _, stats = notebook_summary(output)
            result.update(
                {
                    "status": "reproducible",
                    "sha256": sha256_file(output),
                    **stats,
                }
            )
        except (json.JSONDecodeError, TypeError, ValueError) as error:
            result["reason"] = f"reexecuted notebook is invalid: {error}"
    return result
