# Project-store kernel (inert foundation)

This crate owns engine-independent placement and connection-only bootstrap
metadata. Its production library has no dependency on `codex-core`, `codex-state`,
intelligence or semantic runtime; tests use the state SQLite shim. Native storage
still uses its existing home databases; this foundation does not activate cutover.

Select an existing project explicitly, including a non-Git directory:

```rust,ignore
let project = AbsolutePathBuf::from_absolute_path_checked(project_directory)?;
let location = ProjectStoreLocation::select_project(&project)?;
let database = location.database_path(); // <project>/.stateful/memory.sqlite
```

For implicit selection, pass both the starting directory and a declared workspace
boundary to `ProjectStoreLocation::resolve_workspace`. It discovers an existing
store inside that boundary or chooses the nearest in-boundary Git marker. Plain
directories without a store and nested stores require explicit selection.

Canonical boundary escapes, redirected store components, non-directory projects
and non-regular database paths refuse. Resolution creates no files. Git worktree
marker files are supported; no database is shared through `.git`.

Placement validation does not qualify writable storage or protect against
concurrent same-user filesystem replacement. Openers must revalidate paths,
qualify local unsynchronized storage, negotiate format/domain capabilities and
own SQLite transactions. Neutral reads of knowledge, conversion and native
activation belong to subsequent separately reviewed slices.

`initialize_connection(&mut connection).await?` creates fixed metadata on an empty
caller-owned SQLite connection, or validates the supported bootstrap store.
It returns stable project/store UUIDs only after commit. `read_identity` validates
under a read snapshot and works on a read-only connection. Neither opens files.
The opener must establish fresh-file custody; an unknown preexisting empty file
is not permission to initialize it. Configure a bounded busy timeout first.
Only bootstrap format/domain1 is supported; unknown schemas, changed histories,
partial metadata and nonzero SQLite application/user headers refuse.
