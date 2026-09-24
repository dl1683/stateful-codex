import hashlib
import json
import shutil
import subprocess
import tempfile
import urllib.request
import zipfile
from pathlib import Path
from typing import Any

from protocol import sha256_file

FORBIDDEN_NOTEBOOK_SUFFIXES = {".ipynb", ".qmd", ".rmd"}
FORBIDDEN_NETWORK_HOSTS = (
    "huggingface.co",
    "www.huggingface.co",
    "github.com",
    "raw.githubusercontent.com",
)


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
    """Expose only the capsule's data payload, never its reference notebook."""
    if destination.exists():
        shutil.rmtree(destination)
    destination.mkdir(parents=True)

    with tempfile.TemporaryDirectory(prefix="bixbench-extract-") as directory:
        extract_root = Path(directory)
        with zipfile.ZipFile(archive) as file:
            file.extractall(extract_root)

        data_folders = [
            path
            for path in extract_root.rglob("*")
            if path.is_dir() and "Data" in path.name
        ]
        if len(data_folders) != 1:
            raise ValueError(
                f"{archive.name} must contain exactly one Data directory; "
                f"found {len(data_folders)}"
            )
        data_folder = data_folders[0]
        forbidden = [
            path
            for path in data_folder.rglob("*")
            if path.is_file() and path.suffix.lower() in FORBIDDEN_NOTEBOOK_SUFFIXES
        ]
        if forbidden:
            names = ", ".join(
                path.relative_to(data_folder).as_posix() for path in forbidden
            )
            raise ValueError(f"notebook-like artifacts found inside Data: {names}")

        for item in data_folder.iterdir():
            target = destination / item.name
            if item.is_dir():
                shutil.copytree(item, target)
            else:
                shutil.copy2(item, target)


def workspace_manifest(root: Path) -> dict[str, Any]:
    files = [
        {
            "path": path.relative_to(root).as_posix(),
            "bytes": path.stat().st_size,
            "sha256": sha256_file(path),
        }
        for path in sorted(root.rglob("*"))
        if path.is_file()
    ]
    encoded = json.dumps(files, sort_keys=True, separators=(",", ":")).encode()
    return {
        "files": files,
        "sha256": hashlib.sha256(encoded).hexdigest(),
    }


def parse_usage(log_path: Path) -> dict[str, int] | None:
    latest: dict[str, int] | None = None
    if not log_path.is_file():
        return None
    with log_path.open(encoding="utf-8", errors="replace") as file:
        for line in file:
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            usage = (
                event.get("usage") if event.get("type") == "turn.completed" else None
            )
            if not isinstance(usage, dict):
                continue
            latest = {
                key: value
                for key, value in usage.items()
                if isinstance(value, int) and not isinstance(value, bool)
            }
    return latest


def audit_agent_log(log_path: Path) -> dict[str, Any]:
    findings: list[str] = []
    environment_mutations = (
        "pip install",
        "pip3 install",
        "conda install",
        "mamba install",
        "install.packages(",
        "biocmanager::install(",
    )
    if log_path.is_file():
        with log_path.open(encoding="utf-8", errors="replace") as file:
            for line_number, line in enumerate(file, start=1):
                lowered = line.lower()
                try:
                    event = json.loads(line)
                except json.JSONDecodeError:
                    event = {}
                item = event.get("item")
                command = event.get("command")
                if not isinstance(command, str) and isinstance(item, dict):
                    command = item.get("command")
                command = command.lower() if isinstance(command, str) else ""
                if "web_search_call" in lowered:
                    findings.append(f"line {line_number}: web search event")
                if any(host in lowered for host in FORBIDDEN_NETWORK_HOSTS) and any(
                    command in lowered
                    for command in ("curl", "wget", "invoke-webrequest")
                ):
                    findings.append(
                        f"line {line_number}: benchmark-source network access"
                    )
                if any(mutation in command for mutation in environment_mutations):
                    findings.append(f"line {line_number}: runtime environment mutation")
    return {"passed": not findings, "findings": findings}


def failed_agent_grade(reason: str) -> dict[str, Any]:
    return {
        "status": "agent_failure",
        "official": False,
        "correct": False,
        "reason": reason,
    }


def inspect_image(image: str) -> str:
    completed = subprocess.run(
        ["docker", "image", "inspect", image, "--format", "{{.Id}}"],
        check=True,
        capture_output=True,
        text=True,
    )
    return completed.stdout.strip()


def inspect_environment(image: str) -> dict[str, Any]:
    script = """
import json
import platform
import subprocess

def output(*command):
    return subprocess.check_output(command, text=True, stderr=subprocess.STDOUT).strip()

print(json.dumps({
    "platform": platform.platform(),
    "python": platform.python_version(),
    "r": output("R", "--version").splitlines()[0],
    "condaPackages": json.loads(output("mamba", "list", "--json")),
    "pipFreeze": output("python", "-m", "pip", "freeze").splitlines(),
}, sort_keys=True))
"""
    completed = subprocess.run(
        ["docker", "run", "--rm", image, "python", "-c", script],
        check=True,
        capture_output=True,
        text=True,
    )
    inventory = json.loads(completed.stdout)
    encoded = json.dumps(inventory, sort_keys=True, separators=(",", ":")).encode()
    return {
        "sha256": hashlib.sha256(encoded).hexdigest(),
        "inventory": inventory,
    }
