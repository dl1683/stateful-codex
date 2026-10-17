# Project-local Stateful home

Apply the owner's 2026-10-17 simplification as a path/config change. `scodex`
selects `<project>/.stateful`, with `STATEFUL_STORE_ROOT` as the sole new override.
Resolve native directory options before selecting the Git root or plain folder.
Pass the existing native home/SQLite settings; keep all schemas, filenames,
migrations and locks. Archive the shared home without importing knowledge.
Only login/config seed a private ignored home; publish initial files atomically.
The existing reader gains native read-only access with native eligibility rules;
a short generated README describes schemas, tables and plain SQLite inspection.
Validate with one local jobs2 build, normal scoped tests, final fix/fmt, one
launcher/Luna write/restart/recall check and fresh whole-diff review. Preserve
failures and foreign custody; promote approved work to main and stateful only.
