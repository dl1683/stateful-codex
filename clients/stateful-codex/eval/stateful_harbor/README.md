# Harbor matched-agent evaluation

This adapter compares the branch binary with itself. `BundledCodex` runs
ordinary Codex and `StatefulCodex` adds only `--stateful autonomous`. Both use
the same model, task container, content-addressed bundle, Codex login, Codex
configuration, and Harbor trajectory converter.

The integration is pinned to Harbor commit
`15da91c18580a25489f5bdf2ee71029f3ff3bb2e`. A different Harbor revision is a
protocol change and requires revalidation before a scored run.

## Build the Linux bundle

Run from a Linux checkout or WSL. The worktree must be clean so the SHA-256 can
identify an exact source commit.

```bash
bash clients/stateful-codex/eval/stateful_harbor/build-linux-bundle.sh
```

For faster WSL builds, set `STATEFUL_CODEX_TARGET_DIR` to a directory inside
the Linux filesystem before running the script.

The script builds `codex` and `codex-code-mode-host`, creates a deterministic
package under `clients/stateful-codex/eval/artifacts/`, and writes its SHA-256
sidecar. Binaries are deliberately not committed.

## Configure Harbor

Make the adapter importable and point it to the exact bundle and the normal
Codex login. Do not set an API key for this comparison.

```powershell
$env:PYTHONPATH = "$PWD\clients\stateful-codex\eval"
$env:STATEFUL_CODEX_BUNDLE_PATH = "<absolute path to the .tar.gz>"
$env:STATEFUL_CODEX_BUNDLE_SHA256 = "<64-character digest from .sha256>"
$env:CODEX_AUTH_JSON_PATH = "$env:USERPROFILE\.codex\auth.json"
```

Use `stateful_harbor.stateful_codex:BundledCodex` for the ordinary arm and
`stateful_harbor.stateful_codex:StatefulCodex` for the Stateful arm. Keep the
task list, trial count, model, reasoning effort, timeout, and every other job
setting identical. For example:

```powershell
harbor run -d terminal-bench@2.1 -a stateful_harbor.stateful_codex:BundledCodex -m openai/gpt-5.6-luna --ak reasoning_effort=high
harbor run -d terminal-bench@2.1 -a stateful_harbor.stateful_codex:StatefulCodex -m openai/gpt-5.6-luna --ak reasoning_effort=high
```

Start with the same three to five named tasks in each arm. A pilot is an
engineering diagnostic. An official Terminal-Bench 2.1 comparison requires all
89 tasks and five trials per task for each arm.

Each attempt preserves native Codex logs, an ATIF v1.7 trajectory, adapter
metadata containing the bundle and Harbor hashes, and `final.patch` when the
task workspace is a Git repository. The SQLite state home is outside the
transient Codex session home, so state survives resume or multiple steps inside
one trial. Harbor's fresh task container prevents state from crossing trial
boundaries.
