# Project-store kernel (inert foundation)

This crate owns engine-independent placement, bootstrap metadata and local opening. Its production library has no dependency on `codex-core`, `codex-state`,
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
These bootstrap APIs support only format/domain1; foreign, incomplete or
application-domain stores refuse without automatic adoption.

`qualify_local_filesystem(&path)` checks an existing resolved file or directory.
A root's filesystem does not establish a descendant store directory/file's
filesystem: callers must qualify those actual locations before opening or writing.
The helper creates no state. Known sync paths and unsupported/nonlocal filesystem
kinds refuse; arbitrary sync-agent absence remains a host precondition, not a
claim this heuristic can establish. This alone does not repair caller lifecycle
selection, namespace races, resource admission or native storage routing.

For a host-qualified local unsynchronized project, `initialize_local_store(&location)`
creates metadata in a unique inactive stage, closes its DELETE-mode database,
then publishes it to `memory.sqlite` without replacing an existing file. Competing
initializers return the winning identity. Existing canonical files are inspected
read-only after actual store-directory and database-file filesystem checks; an unknown or empty existing file is refused. `inspect_store(&location)`
never creates a database. SQLite can create sidecars for an existing WAL input.

The `.stateful/.gitignore` policy is atomically published without overwrite.
Fresh Unix state directories use mode0700. Killed initialization may leave ignored
inactive stages; they are never discovered as authority. Cleanup/delivery failure
may follow publication; retry inspects the same identity. Process-kill and power-loss
qualification are separate gates, not inferred from orphan-file tests.

Windows requires a fixed local volume and refuses UNC/network mappings; Linux
and macOS use conservative filesystem allowlists. Recognized sync folders refuse.
Arbitrary sync-agent absence remains a host qualification precondition; this is
not comprehensive sync detection, hostile-SQLite resource qualification or a
security boundary against concurrent same-user replacement. No canonical domain
writer or native attachment is supplied. Writable-copy adoption, WAL domain pools,
legacy conversion and atomic completion remain gated separately.

`PreparedDomains::prepare` accepts trusted application-owned `DomainManifest`
values and constructs an exact format2 schema/history in isolated memory.
Its `read_identity` admits only that complete schema and immutable migration
history, under one read snapshot; a read-only connection is supported. Actual
migration numbers (including gaps) and SHA256 of SQL bytes identify each domain.
Kernel UUIDs/schema and bootstrap migration1 stay unchanged. Manifests must
contain forward, transactional main-database SQL without external effects.
`PreparedDomains::install` explicitly transitions a validated bootstrap connection
to that complete format2 store in one immediate transaction. Repeated installation
requires the identical full manifest/schema. The caller must qualify placement,
enable foreign keys and configure a bounded busy timeout. No WAL pool or native
attachment is supplied. Legacy data import, application eligibility and effective
knowledge reads remain unqualified.
