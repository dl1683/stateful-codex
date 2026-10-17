# Project-local Stateful home

The 2026-10-17 simplification supersedes the combined-store rebuild.
`scodex` resolves `-C`/`--cd`, then the Git root or plain directory; its native
home defaults to `<project>/.stateful`. `STATEFUL_STORE_ROOT` overrides location.
Native filenames, schemas, migrations, locks, sessions and subfolders stay intact.
Fresh homes seed only configuration/login; ignore policies precede private files.
Archive the old shared home with a checked literal-path move and retain it;
never move an active home or use archived knowledge as fallback.

`stateful-memory root/native-read` reads native `project_intelligence_1.sqlite`
with existing eligibility rules. Existing combined-store fixtures still work.
A README beside databases explains tables, schema and plain read-only SQLite.
Validation and retained failures: see `STATEFUL_CODEX_EVALUATION.md`'s store-root
entry and its linked receipts: one local build, normal touched-crate tests,
format/lint, one Luna write/restart/recall check and final whole-diff Droid review.
Merge main/push only `stateful` after approval; no import, exclusion or HAL claim.
