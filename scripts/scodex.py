"""Launch Stateful Codex with its unchanged native home inside the project."""

import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


REPO_ROOT = Path(__file__).resolve().parents[1]
TEMPLATE_HOME = Path.home() / ".codex-stateful"
README = """# Stateful project store

This is the native Stateful Codex home; schemas, migrations and locks are unchanged.
STATEFUL_STORE_ROOT overrides its location (relative to the selected project).
Git and directory-index ignore policies precede private config/login seeding.

Native files include:
- project_intelligence_1.sqlite: hierarchy, context map and blackboard memory.
- stateful_runtime_1.sqlite: runs, obligations and completion accounting.
- state_5.sqlite and thread_history_1.sqlite: project/thread metadata.
- goals_1, memories_1, queue_1 and logs_2.sqlite: other native state.
- sessions/, memories/ and other normal native subfolders.

Memory tables include hierarchy_nodes, context_map_entries, blackboard_entries,
blackboard_entry_revisions, knowledge_context, capture_sources and memory_changes.
Runtime tables include stateful_runs, stateful_run_threads, stateful_obligations,
stateful_steering, stateful_turn_measurements and stateful_acceptance_ledgers.
_sqlx_migrations records versions; inspect actual tables/schema with plain SQLite:

    sqlite3 -readonly project_intelligence_1.sqlite '.tables'
    sqlite3 -readonly project_intelligence_1.sqlite '.schema'
    sqlite3 -readonly project_intelligence_1.sqlite 'SELECT id, project_id, revision FROM blackboard_entries LIMIT 20;'

Any engine can read via SQLite mode=ro. Do not write SQL or use immutable=1 on a
live WAL store; use SQLite backup for live copies. Raw SQL includes historical
and forgotten text. For bounded effective eligible memory, without engine login:

    stateful-memory --project <project> root --json
    stateful-memory --project <project> native-read --project-id <id> --entry <id> --json

Use --store-root <directory> for an explicit reader override. Reads never migrate
or import. Close native writers before moving the home; archives are inactive.
"""


def selected_directory(args, cwd):
    selected = cwd
    arguments = iter(args)
    for arg in arguments:
        if arg in ("--", "memory"):
            break
        if arg in ("-C", "--cd"):
            value = next(arguments, None)
            if value is None:
                raise ValueError(f"{arg} requires a directory")
            selected = Path(value)
        elif arg.startswith("--cd="):
            selected = Path(arg.split("=", 1)[1])
        elif arg.startswith("-C") and len(arg) > 2:
            selected = Path(arg[2:].removeprefix("="))
        elif (
            arg
            in (
                "-c --config -m --model -p --profile -s --sandbox --local-provider "
                "--add-dir --stateful-project --enable --disable --remote "
                "-i --image -o --output-last-message --output-schema --color "
                "-a --ask-for-approval --thread-source"
            ).split()
        ):
            next(arguments, None)
    return (cwd / selected).resolve(strict=True)


def project_directory(directory):
    result = subprocess.run(
        ["git", "-C", str(directory), "rev-parse", "--show-toplevel"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    return (
        Path(result.stdout.removesuffix("\n")).resolve()
        if result.returncode == 0
        else directory
    )


def store_directory(project, environ):
    root = Path(environ.get("STATEFUL_STORE_ROOT") or ".stateful")
    root = (project / root).resolve()
    if (
        root == TEMPLATE_HOME.resolve()
        or root == project
        or root in project.parents
        or (root / ".git").exists()
    ):
        raise ValueError(
            "The store root must be a private directory, not the project or shared template home"
        )
    return root


def publish_missing(destination, source):
    """Publish a complete file without replacing a concurrent initializer's file."""
    descriptor, name = tempfile.mkstemp(dir=destination.parent)
    temporary = Path(name)
    try:
        with os.fdopen(descriptor, "wb") as target:
            shutil.copyfileobj(source, target)
        try:
            os.link(temporary, destination)
        except FileExistsError:
            pass
    finally:
        temporary.unlink()


def prepare_home(root, template):
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    # Ignore private configuration/credentials and all native data before seeding.
    for name in (".gitignore", ".ignore"):
        ignore = root / name
        if not ignore.exists():
            publish_missing(ignore, io.BytesIO(b"*\n"))
        if ignore.read_text(encoding="utf-8") != "*\n":
            raise ValueError(
                f"The store's existing {name} must contain exactly '*' before launch"
            )
    for name in ("config.toml", "auth.json"):
        source, destination = template / name, root / name
        if source.is_file() and not destination.exists():
            with source.open("rb") as seed:
                publish_missing(destination, seed)
    (root / "README.md").write_text(README, encoding="utf-8")


def launch(args):
    project = project_directory(selected_directory(args, Path.cwd()))
    root = store_directory(project, os.environ)
    prepare_home(root, TEMPLATE_HOME)
    environ = os.environ.copy()
    environ.pop("OPENAI_API_KEY", None)
    environ.pop("CODEX_API_KEY", None)
    environ["CODEX_HOME"] = str(root)
    environ["CODEX_SQLITE_HOME"] = str(root)
    binary = (
        REPO_ROOT
        / "codex-rs"
        / "target"
        / "debug"
        / ("codex.exe" if os.name == "nt" else "codex")
    )
    passthrough = {
        "memory",
        "doctor",
        "login",
        "logout",
        "mcp",
        "resume",
        "--version",
        "--help",
    }
    options = []
    option_args = args[: args.index("--")] if "--" in args else args
    if not (args and args[0] in passthrough):
        if not any(
            arg in ("-m", "--model") or arg.startswith(("-m", "--model="))
            for arg in option_args
        ):
            options.extend(["-m", "gpt-6-luna"])
        if not any(
            arg == "--stateful" or arg.startswith("--stateful=") for arg in option_args
        ):
            options.append("--stateful=ask")
    # The native CLI setting wins over a copied legacy sqlite_home preference;
    # managed requirements are still enforced by the native configuration loader.
    quoted_root = json.dumps(str(root), ensure_ascii=False).replace("\x7f", "\\u007f")
    storage = ["-c", f"sqlite_home={quoted_root}"]
    return subprocess.call([str(binary), *storage, *options, *args], env=environ)


if __name__ == "__main__":
    try:
        sys.exit(launch(sys.argv[1:]))
    except (OSError, ValueError) as error:
        print(f"scodex: {error}", file=sys.stderr)
        sys.exit(1)
