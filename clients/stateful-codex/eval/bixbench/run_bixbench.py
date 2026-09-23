import argparse
import json
import shutil
import subprocess
import time
import urllib.request
import uuid
import zipfile
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from protocol import (
    grade_deterministic,
    normalized_answer,
    notebook_summary,
    sha256_file,
)

BIXBENCH_REPOSITORY_URL = "https://huggingface.co/datasets/futurehouse/BixBench"
PROTOCOL_VERSION = 1


def download_verified(url: str, destination: Path, expected_sha256: str) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.is_file() and sha256_file(destination) == expected_sha256:
        return
    temporary = destination.with_suffix(f"{destination.suffix}.download")
    urllib.request.urlretrieve(url, temporary)
    actual = sha256_file(temporary)
    if actual != expected_sha256:
        temporary.unlink(missing_ok=True)
        raise ValueError(
            f"downloaded artifact hash {actual} does not match {expected_sha256}"
        )
    temporary.replace(destination)


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    with path.open(encoding="utf-8") as file:
        return [json.loads(line) for line in file if line.strip()]


def preprocess_capsule(archive: Path, destination: Path) -> None:
    if destination.exists():
        shutil.rmtree(destination)
    destination.mkdir(parents=True)
    with zipfile.ZipFile(archive) as file:
        file.extractall(destination)

    data_folders = [
        path for path in destination.rglob("*") if path.is_dir() and "Data" in path.name
    ]
    if not data_folders:
        raise FileNotFoundError(f"{archive.name} has no Data directory")
    data_folder = data_folders[0]
    for item in data_folder.iterdir():
        target = destination / item.name
        if item.is_dir():
            shutil.copytree(item, target, dirs_exist_ok=True)
        else:
            shutil.copy2(item, target)

    for path in sorted(
        destination.rglob("*"), key=lambda item: len(item.parts), reverse=True
    ):
        if path.is_dir() and ("Data" in path.name or "Notebook" in path.name):
            shutil.rmtree(path)
    for notebook in destination.glob("*.ipynb"):
        notebook.unlink()


def prompt_for(question: dict[str, Any]) -> str:
    return f"""You are the analysis agent in a BixBench v1.5 evaluation.

The project directory /workspace contains the input data for exactly one
bioinformatics question. Analyze the files directly and answer this question:

<question>
{question["question"]}
</question>

Required deliverables:
1. Create /workspace/notebook.ipynb as a valid, executed Jupyter notebook.
2. Use small or medium Python code cells. Preserve the code, tables, statistics,
   and cell outputs needed for an independent reviewer to verify the answer.
3. Inspect the data rigorously, check shapes and missing values, and state any
   scientifically important assumptions in code comments and printed output.
4. Return one concise answer in the required JSON response. For a numerical
   question, the answer field must contain only the number, without units or
   explanatory prose. Put explanation in summary and evidence instead.
5. Before finishing, reopen notebook.ipynb and verify that it is valid and that
   its recorded outputs support the answer.

Do not search for or infer a benchmark answer key. Solve the question from the
project data. Work autonomously until both the notebook and answer are complete.
"""


def write_control_files(control_dir: Path, question: dict[str, Any]) -> None:
    control_dir.mkdir(parents=True)
    (control_dir / "prompt.txt").write_text(prompt_for(question), encoding="utf-8")
    schema = {
        "type": "object",
        "additionalProperties": False,
        "required": ["answer", "summary", "evidence"],
        "properties": {
            "answer": {"type": "string"},
            "summary": {"type": "string"},
            "evidence": {
                "type": "array",
                "maxItems": 5,
                "items": {"type": "string"},
            },
        },
    }
    (control_dir / "final.schema.json").write_text(
        json.dumps(schema, indent=2), encoding="utf-8"
    )


def docker_mount(source: Path, target: str, *, readonly: bool = False) -> list[str]:
    spec = f"type=bind,source={source.resolve()},target={target}"
    if readonly:
        spec += ",readonly"
    return ["--mount", spec]


def parse_usage(log_path: Path) -> dict[str, int] | None:
    usage = None
    if not log_path.is_file():
        return None
    with log_path.open(encoding="utf-8", errors="replace") as file:
        for line in file:
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if event.get("type") == "turn.completed":
                usage = event.get("usage")
    return usage


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
    timeout_seconds: int,
) -> dict[str, Any]:
    task_id = question["question_id"]
    task_root = run_root / "tasks" / task_id
    workspace = task_root / "workspace"
    state = task_root / "state"
    logs = task_root / "logs"
    control = task_root / "control"
    shutil.copytree(capsule_root, workspace)
    state.mkdir(parents=True)
    logs.mkdir(parents=True)
    write_control_files(control, question)

    container_name = f"stateful-bixbench-{task_id}-{uuid.uuid4().hex[:8]}"
    command = ["docker", "run", "--rm", "--name", container_name]
    command += docker_mount(workspace, "/workspace")
    command += docker_mount(state, "/state")
    command += docker_mount(logs, "/logs")
    command += docker_mount(control, "/control", readonly=True)
    command += docker_mount(bundle, "/input/stateful-codex.tar.gz", readonly=True)
    command += docker_mount(auth, "/input/auth.json", readonly=True)
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
codex exec \
  --dangerously-bypass-approvals-and-sandbox \
  --skip-git-repo-check \
  --ignore-user-config \
  --model "$BIXBENCH_MODEL" \
  --json \
  --enable unified_exec \
  -c "model_reasoning_effort=$BIXBENCH_EFFORT" \
  -c 'sqlite_home="/state"' \
  --stateful autonomous \
  --output-schema /control/final.schema.json \
  --output-last-message /logs/final.json \
  - < /control/prompt.txt > /logs/codex.jsonl 2> /logs/codex.stderr
""",
    ]
    environment = {
        "BIXBENCH_MODEL": model,
        "BIXBENCH_EFFORT": effort,
    }
    for key, value in environment.items():
        command[3:3] = ["--env", f"{key}={value}"]

    started = time.monotonic()
    timed_out = False
    exit_code = None
    try:
        completed = subprocess.run(command, timeout=timeout_seconds, check=False)
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
        "statefulMode": "autonomous",
        "stateScope": "question",
        "image": image,
        "imageId": image_id,
        "bundleSha256": bundle_sha256,
        "usage": parse_usage(logs / "codex.jsonl"),
    }
    final_path = logs / "final.json"
    notebook_path = workspace / "notebook.ipynb"
    if (
        exit_code == 0
        and not timed_out
        and final_path.is_file()
        and notebook_path.is_file()
    ):
        final = json.loads(final_path.read_text(encoding="utf-8"))
        answer = normalized_answer(final["answer"])
        notebook, stats = notebook_summary(notebook_path)
        result.update(
            {
                "answer": answer,
                "summary": final["summary"],
                "evidence": final["evidence"],
                "grade": grade_deterministic(
                    question["eval_mode"], question["ideal"], answer
                ),
                "notebook": {
                    **stats,
                    "sha256": sha256_file(notebook_path),
                },
            }
        )
        trajectory = {
            "problem_id": task_id,
            "agent_answer": {"answer": answer},
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
    else:
        result["grade"] = {
            "status": "not_graded",
            "correct": None,
            "reason": "agent or deliverable failure",
        }
    (logs / "result.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
    return result


def inspect_image(image: str) -> str:
    completed = subprocess.run(
        ["docker", "image", "inspect", image, "--format", "{{.Id}}"],
        check=True,
        capture_output=True,
        text=True,
    )
    return completed.stdout.strip()


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
    parser.add_argument("--timeout-seconds", type=int, default=3600)
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
    run_root = args.output_dir
    run_manifest = {
        "schemaVersion": 1,
        "protocolVersion": PROTOCOL_VERSION,
        "name": manifest["name"],
        "startedAt": datetime.now(UTC).isoformat(),
        "upstream": manifest["upstream"],
        "dataset": dataset,
        "capsules": manifest["capsules"],
        "tasks": manifest["tasks"],
        "stateScope": "question",
        "image": args.image,
        "imageId": image_id,
        "bundleSha256": args.bundle_sha256,
        "model": args.model,
        "effort": args.effort,
        "timeoutSeconds": args.timeout_seconds,
        "concurrency": args.concurrency,
    }
    (run_root / "run-manifest.json").write_text(
        json.dumps(run_manifest, indent=2), encoding="utf-8"
    )

    results = []
    with ThreadPoolExecutor(max_workers=args.concurrency) as executor:
        futures = {
            executor.submit(
                run_task,
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
                timeout_seconds=args.timeout_seconds,
            ): question["question_id"]
            for question in selected
        }
        for future in as_completed(futures):
            try:
                results.append(future.result())
            except Exception as error:  # noqa: BLE001
                # Preserve one task's harness failure without cancelling its peers.
                results.append(
                    {
                        "taskId": futures[future],
                        "grade": {
                            "status": "harness_error",
                            "correct": None,
                            "reason": str(error),
                        },
                    }
                )

    results.sort(key=lambda result: manifest["tasks"].index(result["taskId"]))
    summary = {
        **run_manifest,
        "completedAt": datetime.now(UTC).isoformat(),
        "results": results,
        "passed": sum(
            result.get("grade", {}).get("correct") is True for result in results
        ),
        "failed": sum(
            result.get("grade", {}).get("correct") is False for result in results
        ),
        "ungraded": sum(
            result.get("grade", {}).get("correct") is None for result in results
        ),
    }
    (run_root / "summary.json").write_text(
        json.dumps(summary, indent=2), encoding="utf-8"
    )
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
