"""Launch Stateful Codex with its unchanged native home inside the project."""

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

This directory is the native Stateful Codex home. Keep it private and out of Git.
Git and directory-index ignore policies are written before private data is seeded.
Its layout, SQLite schemas, migrations and locks are the existing Codex layout;
no combined memory.sqlite or legacy import is required. STATEFUL_STORE_ROOT is
the optional location override (relative paths resolve from the selected project).

project_intelligence_1.sqlite holds hierarchy, context-map and blackboard memory;
stateful_runtime_1.sqlite holds runs, obligations and completion accounting.
state_5.sqlite and thread_history_1.sqlite hold native project/thread metadata.
Other native databases include goals_1, memories_1, queue_1 and logs_2.sqlite.
sessions/, memories/ and other subfolders retain their normal native roles.
Each database's _sqlx_migrations table records its schema version. List the
actual tables with plain SQLite; future versions may add native tables:

Memory tables include hierarchy_nodes, context_map_entries, blackboard_entries,
blackboard_entry_revisions, knowledge_context, capture_sources and memory_changes.
Runtime tables include stateful_runs, stateful_run_threads, stateful_obligations,
stateful_steering, stateful_turn_measurements and stateful_acceptance_ledgers.

    sqlite3 -readonly project_intelligence_1.sqlite '.tables'
    sqlite3 -readonly project_intelligence_1.sqlite '.schema'
    sqlite3 -readonly project_intelligence_1.sqlite 'SELECT id, project_id, revision FROM blackboard_entries LIMIT 20;'

Any engine can inspect these databases with SQLite mode=ro. Do not write via SQL,
copy a live database without SQLite backup, or use immutable=1 on a live WAL store.
Native writers retain their existing SQLite locking; close them before moving
the store. Archives are inactive snapshots, never fallback authorities.
Raw SQL exposes historical/forgotten text. For effective eligible memory use:

    stateful-memory --project <project> root --json
    stateful-memory --project <project> native-read --project-id <id> --entry <id> --json

Pass --store-root <directory> to that reader for an explicit root override.
The reader opens existing files read-only, never migrates or imports, and applies
the same eligibility/retirement rules as native exact reads. Its bounded replies
refuse oversized results. Hosted model use does not establish offline readiness.
"""


def selected_directory(args, cwd):
    selected = cwd
    for index, arg in enumerate(args):
        if arg == "--":
            break
        if arg in ("-C", "--cd"):
            if index + 1 == len(args):
                raise ValueError(f"{arg} requires a directory")
            selected = Path(args[index + 1])
        elif arg.startswith("--cd="):
            selected = Path(arg.split("=", 1)[1])
        elif arg.startswith("-C") and len(arg) > 2:
            selected = Path(arg[2:].removeprefix("="))
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
        Path(result.stdout.strip()).resolve() if result.returncode == 0 else directory
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


def prepare_home(root, template):
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    # Ignore private configuration/credentials and all native data before seeding.
    for name in (".gitignore", ".ignore"):
        ignore = root / name
        if ignore.exists() and ignore.read_text(encoding="utf-8") != "*\n":
            raise ValueError(
                f"The store's existing {name} must contain exactly '*' before launch"
            )
        ignore.write_text("*\n", encoding="utf-8")
    for name in ("config.toml", "auth.json"):
        source, destination = template / name, root / name
        if source.is_file() and not destination.exists():
            # Publish complete seeds atomically without overwriting another launch.
            with tempfile.NamedTemporaryFile(dir=root, delete=False) as target:
                temporary = Path(target.name)
                with source.open("rb") as seed:
                    shutil.copyfileobj(seed, target)
            try:
                os.link(temporary, destination)
            except FileExistsError:
                pass
            finally:
                temporary.unlink()
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
    options = [] if args and args[0] in passthrough else ["-m", "gpt-6-luna"]
    if options and not any(
        arg == "--stateful" or arg.startswith("--stateful=") for arg in args
    ):
        options.append("--stateful")
    # The native CLI setting wins over a copied legacy sqlite_home preference;
    # managed requirements are still enforced by the native configuration loader.
    storage = ["-c", f"sqlite_home={json.dumps(str(root), ensure_ascii=False)}"]
    return subprocess.call([str(binary), *storage, *options, *args], env=environ)


if __name__ == "__main__":
    try:
        sys.exit(launch(sys.argv[1:]))
    except (OSError, ValueError) as error:
        print(f"scodex: {error}", file=sys.stderr)
        sys.exit(1)
