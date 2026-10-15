# Project-store kernel (inert foundation)

This crate owns engine-independent project-store placement. It has no dependency
on `codex-core`, `codex-state`, intelligence or semantic runtime. Native storage
still uses its existing home databases; this resolver does not activate cutover.

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
own SQLite transactions. Store UUIDs, neutral reads, conversion and native
activation belong to subsequent separately reviewed slices.
