import argparse
import json
import shutil
import subprocess
import time
import uuid
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from artifact_validation import (
    assess_operational_validity,
    classify_provider_failure,
    inspect_stateful_state,
    reexecute_notebook,
)
from protocol import (
    grade_deterministic,
    normalized_answer,
    notebook_summary,
    prompt_for,
    sha256_file,
    write_control_files,
)
from runner_support import (
    FORBIDDEN_NETWORK_HOSTS,
    audit_agent_log,
    download_verified,
    failed_agent_grade,
    inspect_environment,
    inspect_image,
    load_jsonl,
    parse_usage,
    preprocess_capsule,
    workspace_manifest,
)

BIXBENCH_REPOSITORY_URL = "https://huggingface.co/datasets/futurehouse/BixBench"
PROTOCOL_VERSION = 1


def docker_mount(source: Path, target: str, *, readonly: bool = False) -> list[str]:
    spec = f"type=bind,source={source.resolve()},target={target}"
    if readonly:
        spec += ",readonly"
    return ["--mount", spec]


def remove_container(name: str) -> None:
    subprocess.run(
        ["docker", "rm", "--force", name],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )


def run_task(
    question: dict[str, Any],
    *,
    run_root: Path,
    capsule_root: Path,
    image: str,
    image_id: str,
    bundle: Path,
    bundle_sha256: str,
    auth: Path,
    model: str,
    effort: str,
    arm: str,
    timeout_seconds: int,
    notebook_timeout_seconds: int,
) -> dict[str, Any]:
    task_id = question["question_id"]
    task_root = run_root / "tasks" / task_id
    workspace = task_root / "workspace"
    state = task_root / "state"
    sessions = task_root / "sessions"
    logs = task_root / "logs"
    control = task_root / "control"
    shutil.copytree(capsule_root, workspace)
    state.mkdir(parents=True)
    sessions.mkdir(parents=True)
    logs.mkdir(parents=True)
    control_hashes = write_control_files(control, question)
    input_manifest = workspace_manifest(workspace)
    (task_root / "workspace-manifest.json").write_text(
        json.dumps(input_manifest, indent=2), encoding="utf-8"
    )

    container_name = f"stateful-bixbench-{task_id}-{uuid.uuid4().hex[:8]}"
    command = ["docker", "run", "--rm", "--name", container_name]
    command += docker_mount(workspace, "/workspace")
    command += docker_mount(state, "/state")
    command += docker_mount(sessions, "/codex-home/sessions")
    command += docker_mount(logs, "/logs")
    command += docker_mount(control, "/control", readonly=True)
    command += docker_mount(bundle, "/input/stateful-codex.tar.gz", readonly=True)
    command += docker_mount(auth, "/input/auth.json", readonly=True)
    for host in FORBIDDEN_NETWORK_HOSTS:
        command += ["--add-host", f"{host}:0.0.0.0"]
    command += [
        image,
        "bash",
        "-lc",
        """set -euo pipefail
unset OPENAI_API_KEY CODEX_API_KEY
install -d -m 0755 /opt/stateful-codex /codex-home
tar -xzf /input/stateful-codex.tar.gz -C /opt/stateful-codex --no-same-owner
install -m 0600 /input/auth.json /codex-home/auth.json
ln -sfn /opt/stateful-codex/bin/codex /usr/local/bin/codex
ln -sfn /opt/stateful-codex/bin/codex-code-mode-host /usr/local/bin/codex-code-mode-host
export CODEX_HOME=/codex-home
cd /workspace
printf '{"started":true,"at":"%s"}\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > /logs/agent-started.json
stateful_args=()
if [[ "$BIXBENCH_ARM" == "stateful" ]]; then
  stateful_args=(--stateful autonomous)
fi
codex exec \
  --dangerously-bypass-approvals-and-sandbox \
  --skip-git-repo-check \
  --ignore-user-config \
  --model "$BIXBENCH_MODEL" \
  --json \
  --enable unified_exec \
  --disable memories \
  -c "model_reasoning_effort=$BIXBENCH_EFFORT" \
  -c 'sqlite_home="/state"' \
  -c 'web_search="disabled"' \
  -c 'memories.generate_memories=false' \
  -c 'memories.use_memories=false' \
  "${stateful_args[@]}" \
  --output-schema /control/final.schema.json \
  --output-last-message /logs/final.json \
  - < /control/prompt.txt > /logs/codex.jsonl 2> /logs/codex.stderr
""",
    ]
    environment = {
        "BIXBENCH_MODEL": model,
        "BIXBENCH_EFFORT": effort,
        "BIXBENCH_ARM": arm,
    }
    for key, value in environment.items():
        command[3:3] = ["--env", f"{key}={value}"]

    started = time.monotonic()
    timed_out = False
    exit_code = None
    with (
        (logs / "container.stdout").open("wb") as stdout,
        (logs / "container.stderr").open("wb") as stderr,
    ):
        try:
            completed = subprocess.run(
                command,
                timeout=timeout_seconds,
                check=False,
                stdout=stdout,
                stderr=stderr,
            )
            exit_code = completed.returncode
        except subprocess.TimeoutExpired:
            timed_out = True
            remove_container(container_name)
    duration = time.monotonic() - started

    result: dict[str, Any] = {
        "taskId": task_id,
        "capsuleUuid": question["capsule_uuid"],
        "evalMode": question["eval_mode"],
        "question": question["question"],
        "exitCode": exit_code,
        "timedOut": timed_out,
        "durationSeconds": round(duration, 3),
        "model": model,
        "effort": effort,
        "arm": arm,
        "statefulMode": "autonomous" if arm == "stateful" else None,
        "stateScope": "question",
        "image": image,
        "imageId": image_id,
        "bundleSha256": bundle_sha256,
        "usage": parse_usage(logs / "codex.jsonl"),
        "control": control_hashes,
        "workspaceManifestSha256": input_manifest["sha256"],
        "integrityAudit": audit_agent_log(logs / "codex.jsonl"),
    }
    final_path = logs / "final.json"
    notebook_path = workspace / "notebook.ipynb"
    final = None
    notebook = None
    stats = None
    agent_started = (logs / "agent-started.json").is_file()
    result["agentStarted"] = agent_started
    if not agent_started:
        result["grade"] = {
            "status": "invalid_infrastructure",
            "official": False,
            "correct": None,
            "reason": "container did not reach the Codex invocation boundary",
        }
    elif timed_out:
        result["grade"] = failed_agent_grade("agent timed out")
    elif exit_code != 0:
        provider_failure = classify_provider_failure(
            logs / "codex.stderr", logs / "codex.jsonl"
        )
        result["grade"] = (
            {
                "status": "invalid_provider",
                "official": False,
                "correct": None,
                "reason": provider_failure,
            }
            if provider_failure
            else failed_agent_grade(f"agent exited with code {exit_code}")
        )
    elif not final_path.is_file():
        result["grade"] = failed_agent_grade(
            "agent produced no structured final answer"
        )
    else:
        try:
            final = json.loads(final_path.read_text(encoding="utf-8"))
            answer = normalized_answer(final["answer"])
            result.update(
                {
                    "answer": answer,
                    "summary": final["summary"],
                    "evidence": final["evidence"],
                    "grade": grade_deterministic(
                        question["eval_mode"], question["ideal"], answer
                    ),
                }
            )
        except (json.JSONDecodeError, KeyError, TypeError, ValueError) as error:
            result["grade"] = failed_agent_grade(
                f"malformed structured final answer: {error}"
            )

    if notebook_path.is_file():
        try:
            notebook, stats = notebook_summary(notebook_path)
            result["notebook"] = {
                "status": "valid",
                **stats,
                "sha256": sha256_file(notebook_path),
            }
        except (json.JSONDecodeError, TypeError, ValueError) as error:
            result["notebook"] = {
                "status": "invalid",
                "reason": str(error),
                "sha256": sha256_file(notebook_path),
            }
    else:
        result["notebook"] = {
            "status": "missing",
            "reason": "agent produced no notebook.ipynb",
        }
    result["notebookReplay"] = (
        reexecute_notebook(
            workspace,
            task_root,
            image,
            notebook_timeout_seconds,
        )
        if result["notebook"]["status"] == "valid"
        else {
            "status": "skipped",
            "reason": "the submitted notebook is not valid",
        }
    )
    result["statefulState"] = (
        inspect_stateful_state(state)
        if arm == "stateful"
        else {"status": "not_applicable", "valid": None}
    )

    if not result["integrityAudit"]["passed"]:
        result["grade"] = {
            "status": "invalid_protocol",
            "official": False,
            "correct": None,
            "reason": "agent attempted prohibited benchmark-source access",
        }

    result["operationalValidity"] = assess_operational_validity(
        result, stateful=arm == "stateful"
    )

    if final is not None:
        trajectory = {
            "problem_id": task_id,
            "agent_answer": {"answer": result.get("answer")},
            "ideal_answer": question["ideal"],
            "problem": prompt_for(question),
            "mcq_options": question["distractors"],
            "mcq_question": question["question"],
            "notebook_stats": stats,
            "num_actions": None,
            "question_format": "open",
            "refusal_option": False,
            "model": model,
            "metadata": {
                "capsule_uuid": question["capsule_uuid"],
                "data_folder": question["data_folder"],
                "distractors": question["distractors"],
                "eval_mode": question["eval_mode"],
                "question_id": task_id,
            },
            "nb": notebook,
            "run_name": run_root.name,
        }
        trajectory_dir = run_root / "trajectories"
        trajectory_dir.mkdir(exist_ok=True)
        (trajectory_dir / f"{task_id}_replica_0.json").write_text(
            json.dumps(trajectory, indent=2), encoding="utf-8"
        )
    (logs / "result.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
    return result


def run_task_safely(question: dict[str, Any], **kwargs: Any) -> dict[str, Any]:
    try:
        return run_task(question, **kwargs)
    except Exception as error:  # noqa: BLE001
        result = {
            "taskId": question["question_id"],
            "grade": {
                "status": "invalid_infrastructure",
                "official": False,
                "correct": None,
                "reason": f"runner error: {error}",
            },
        }
        logs = kwargs["run_root"] / "tasks" / question["question_id"] / "logs"
        logs.mkdir(parents=True, exist_ok=True)
        (logs / "result.json").write_text(
            json.dumps(result, indent=2), encoding="utf-8"
        )
        return result


def main() -> None:
    parser = argparse.ArgumentParser(description="Run Stateful Codex on BixBench")
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--cache-dir", type=Path)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--bundle-sha256", required=True)
    parser.add_argument(
        "--auth", type=Path, default=Path.home() / ".codex" / "auth.json"
    )
    parser.add_argument("--image", default="stateful-bixbench-agent:fhda-v1.5.0-amd64")
    parser.add_argument("--model", default="gpt-5.6-luna")
    parser.add_argument("--effort", default="max")
    parser.add_argument("--arm", choices=("ordinary", "stateful"), default="stateful")
    parser.add_argument("--timeout-seconds", type=int, default=3600)
    parser.add_argument("--notebook-timeout-seconds", type=int, default=900)
    parser.add_argument("--concurrency", type=int, default=1)
    args = parser.parse_args()

    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    if manifest.get("schemaVersion") != 1:
        raise ValueError("unsupported manifest schemaVersion")
    if manifest.get("stateScope") != "question":
        raise ValueError("this protocol version supports only question-isolated state")
    if not args.bundle.is_file() or sha256_file(args.bundle) != args.bundle_sha256:
        raise ValueError("bundle is missing or does not match --bundle-sha256")
    if not args.auth.is_file():
        raise FileNotFoundError("Codex auth.json was not found")
    if args.concurrency < 1:
        raise ValueError("--concurrency must be positive")
    if args.notebook_timeout_seconds < 1:
        raise ValueError("--notebook-timeout-seconds must be positive")

    args.output_dir.mkdir(parents=True, exist_ok=False)
    cache_dir = args.cache_dir or args.output_dir / "cache"
    dataset = manifest["dataset"]
    metadata_path = cache_dir / "BixBench.jsonl"
    metadata_url = (
        f"{BIXBENCH_REPOSITORY_URL}/resolve/{dataset['revision']}/BixBench.jsonl"
        "?download=true"
    )
    download_verified(metadata_url, metadata_path, dataset["metadataSha256"])
    by_id = {row["question_id"]: row for row in load_jsonl(metadata_path)}

    selected = []
    capsule_roots: dict[str, Path] = {}
    for task_id in manifest["tasks"]:
        question = by_id.get(task_id)
        if question is None:
            raise KeyError(f"unknown BixBench question {task_id}")
        archive_name = question["data_folder"]
        expected = manifest["capsules"].get(archive_name)
        if not expected:
            raise KeyError(f"manifest has no hash for {archive_name}")
        archive = cache_dir / archive_name
        archive_url = (
            f"{BIXBENCH_REPOSITORY_URL}/resolve/{dataset['revision']}/{archive_name}"
            "?download=true"
        )
        download_verified(archive_url, archive, expected)
        capsule_root = cache_dir / "capsules" / question["capsule_uuid"]
        if question["capsule_uuid"] not in capsule_roots:
            preprocess_capsule(archive, capsule_root)
            capsule_roots[question["capsule_uuid"]] = capsule_root
        selected.append(question)

    image_id = inspect_image(args.image)
    environment = inspect_environment(args.image)
    run_root = args.output_dir
    (run_root / "environment-manifest.json").write_text(
        json.dumps(environment, indent=2), encoding="utf-8"
    )
    harness_root = Path(__file__).resolve().parent
    harness_files = (
        "Dockerfile.agent",
        "artifact_validation.py",
        "protocol.py",
        "run_bixbench.py",
        "runner_support.py",
    )
    run_manifest = {
        "schemaVersion": 1,
        "protocolVersion": PROTOCOL_VERSION,
        "scoreKind": "local_metadata_verifier",
        "name": manifest["name"],
        "startedAt": datetime.now(UTC).isoformat(),
        "upstream": manifest["upstream"],
        "dataset": dataset,
        "capsules": manifest["capsules"],
        "tasks": manifest["tasks"],
        "stateScope": "question",
        "image": args.image,
        "imageId": image_id,
        "environmentManifestSha256": environment["sha256"],
        "bundleSha256": args.bundle_sha256,
        "selectionManifestSha256": sha256_file(args.manifest),
        "harnessSha256": {
            name: sha256_file(harness_root / name) for name in harness_files
        },
        "model": args.model,
        "effort": args.effort,
        "arm": args.arm,
        "timeoutSeconds": args.timeout_seconds,
        "notebookTimeoutSeconds": args.notebook_timeout_seconds,
        "concurrency": args.concurrency,
    }
    (run_root / "run-manifest.json").write_text(
        json.dumps(run_manifest, indent=2), encoding="utf-8"
    )

    results = []
    with ThreadPoolExecutor(max_workers=args.concurrency) as executor:
        futures = {
            executor.submit(
                run_task_safely,
                question,
                run_root=run_root,
                capsule_root=capsule_roots[question["capsule_uuid"]],
                image=args.image,
                image_id=image_id,
                bundle=args.bundle,
                bundle_sha256=args.bundle_sha256,
                auth=args.auth,
                model=args.model,
                effort=args.effort,
                arm=args.arm,
                timeout_seconds=args.timeout_seconds,
                notebook_timeout_seconds=args.notebook_timeout_seconds,
            ): question["question_id"]
            for question in selected
        }
        for future in as_completed(futures):
            results.append(future.result())

    results.sort(key=lambda result: manifest["tasks"].index(result["taskId"]))
    summary = {
        **run_manifest,
        "completedAt": datetime.now(UTC).isoformat(),
        "results": results,
        "localVerifierCorrect": sum(
            result.get("grade", {}).get("status") == "metadata_verifier"
            and result.get("grade", {}).get("correct") is True
            for result in results
        ),
        "localVerifierIncorrect": sum(
            result.get("grade", {}).get("status") == "metadata_verifier"
            and result.get("grade", {}).get("correct") is False
            for result in results
        ),
        "requiresOfficialGrading": sum(
            result.get("grade", {}).get("status")
            == "requires_official_llm_grader"
            for result in results
        ),
        "agentFailures": sum(
            result.get("grade", {}).get("status") == "agent_failure"
            for result in results
        ),
        "invalidRuns": sum(
            str(result.get("grade", {}).get("status", "")).startswith("invalid_")
            for result in results
        ),
        "operationallyValid": sum(
            result.get("operationalValidity", {}).get("valid") is True
            for result in results
        ),
        "operationallyInvalid": sum(
            result.get("operationalValidity", {}).get("valid") is False
            for result in results
        ),
        "reproducibleNotebooks": sum(
            result.get("notebookReplay", {}).get("status") == "reproducible"
            for result in results
        ),
        "completedStatefulRuns": sum(
            result.get("statefulState", {}).get("valid") is True for result in results
        ),
    }
    (run_root / "summary.json").write_text(
        json.dumps(summary, indent=2), encoding="utf-8"
    )
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
