# Project-local Stateful home

The 2026-10-17 simplification directive supersedes the combined-database rebuild.
Keep every existing schema, filename, migration and locking mechanism. Select a
native Codex home at `<project>/.stateful`, with `STATEFUL_STORE_ROOT` as the single
location override. The launcher resolves `-C`/`--cd` before selecting the Git root
or plain directory, then runs the existing binary with that home. This moves
SQLite databases, sessions, memory files and runtime subfolders together without
changing domain code. Engine login/configuration seed a fresh home; knowledge is
never imported. Private home contents are ignored before they are created.

Archive the old shared home by a checked, literal-path rename; retain credentials
and configuration as launcher defaults, with no shared knowledge fallback.
Existing processes keep their custody; never move a home while a process uses it.
The read-only `stateful-memory native-root/native-read` interface targets the
unchanged `project_intelligence_1.sqlite` and uses existing effective-read rules.
The earlier combined-database reader remains supported for its existing fixtures.
A short generated README beside the databases describes plain SQLite inspection.

Validation: launcher root/override/argument and home-isolation tests; native-reader
integration tests including restart and retirement; one serialized local CLI and
reader build with jobs=2 and the existing RAM floor; the reader crate's normal
tests; final format; a launcher/Luna scratch memory write/restart/read journey;
one fresh Droid whole-diff review. Publish to main and `stateful` after approval.
No schema redesign, import, old-client barrier, component qualification or HAL
acceptance claim is part of this change.
