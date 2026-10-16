# Next S1 read boundary: admitted snapshots before effective knowledge

Design only; no implementation or milestone acceptance. Authority remains the
converged rebuild strategy. Format2 preparation/installation and the owned reader
are landed; native stores, caches, completion and conversion remain unchanged.

## Source constraint and selected stages

ProjectStoreReader currently validates schema/history/identity in one transaction,
then returns a raw owned read-only connection to trusted domain code. Its docs
correctly assign subsequent snapshots and eligibility to that caller. Admission
does not pin future reads to its original snapshot. PreparedDomains::validate is
private; read_identity starts/commits its own transaction. A facade must not copy
kernel validation SQL or pretend that a second transaction shares that snapshot.

Stage1 adds an additive owned domain reader plus a transaction guard in
codex-project-store. The domain manifest must match exactly as it does today.
The owner retains PreparedDomains, the admission identity and one connection;
no pool, reconnect, home or path reopen. begin_snapshot starts one read transaction,
runs the existing complete bounded schema/history/header/identity validation on
that transaction, and compares both UUIDs with admission. Only then return a guard
that trusted domain code can borrow as a mutable SqliteConnection. Borrowing the
guard prevents another operation on that owner. The guard owns the transaction,
not just a connection borrow; trusted borrowers must not commit/rollback/detach it.
Explicit finish/close awaits transaction completion; validation refusal awaits
rollback. Begin/finish cancellation or an unfinished dropped guard makes its owner
close-only: no reuse until the connection has been explicitly closed. SQLx's
queued rollback alone does not satisfy that reuse or move contract. No external work or
model inference occurs while a guard is held. Existing raw trusted API remains
compatible and makes no stronger promise. No new dependency or native route.

Stage1 fixtures: mutation between admission and begin must refuse changed kernel,
schema or history; exact identity preserved on success; concurrent writer after
the snapshot starts must not mix generations within that snapshot; a new snapshot
observes the committed change/refusal; readonly8; explicit release/move/reopen.
Use deterministic independent connections and, separately, forced process barriers
for any process-concurrency claim. No power-loss or namespace security claim.

Stage2 stays in project-intelligence. First freeze a behavior-preserving internal
extraction; bounded projection/facade behavior is a separate review unit. Add an owned project knowledge facade that
consumes that domain owner with the application's exact compiled manifest.
Keep both legacy home constructors. Factor the existing get_hit transaction body
into a crate-private connection-based operation shared by native and neutral
callers; never replace it with get_entry or duplicate eligibility SQL in a CLI.
The facade exposes ModelTool policy only, never a caller-selected authority enum.
Reuse identity::entry_source_eligible_on, coverage_available and hit construction;
review all relation/premise/context paths rather than treating the initial entry
predicate as sufficient for every nested payload. Large query modules get a new
module, with related tests beside it, instead of more orchestration methods.

For fresh stores only, choose the canonical kernel project UUID string as the
domain project_id. The facade derives it, never accepts a caller project selector.
Fixtures must establish this binding explicitly; refuse populated projects whose
domain ownership is incompatible rather than selecting the first project row.
Check relevant direct and reachable domain ownership within each read snapshot;
absence of a coverage row does not establish ownership. Kernel IDs versus legacy project IDs remain an S3 mapping/conversion decision;
no automatic rewrite or claim that existing source-derived IDs equal UUIDs.

The facade's first public operation is a bounded exact entry projection, not a
root or search page, after the separate field/projection freeze below.
Read identity, revision and capsule eligibility generation in the same guarded
snapshot as eligibility/content. Missing supported clock rows may mean zero only
under the existing fresh-store contract; inspect SQLite storage types and ranges
before converting, because these clock tables are not STRICT. Malformed,
negative or overflowing values refuse.
Round-trip clocks as decimal strings at the eventual wire boundary. A result
describes its read snapshot; a later Forget cannot retract already delivered data.
The next operation begins a fresh snapshot, so committed retirement/exclusion/
coverage changes apply. Continuations and persisted cache invalidation are later.

The existing full load_hit includes relations and recursive premise evidence;
wrapping it and truncating serialized JSON would not bound allocations or preserve
complete semantics. Before a public wire command, choose and review bounded
scalar/row projections for every exposed field and child collection, applying the
same eligibility before limits. Oversized requested content returns an explicit
bounded omission/error, never an unlabelled prefix. Preserve historical bytes;
do not mutate knowledge to make it fit. This is a prerequisite, not a claim that
the existing loader already supplies those bounds.

Legacy get_hit currently returns None when incomplete coverage hides an entry;
its behavior-preserving extraction must keep that result. The neutral facade
reports incomplete coverage separately from an ordinary miss, using the same
snapshot. Compare equivalent payload projections for parity and assert legacy
compatibility separately; do not demand equal envelopes with new facade metadata.
The subsequent field freeze must explicitly decide nested omission/refusal
semantics: recursive premise evidence does not already share all relation-endpoint
eligibility checks. Omitted or unsupported support must never improve effective
verification. Neither this design nor Stage1 approves that unwritten projection.

Stage3 adds a separate auth-free CLI exact read, then root/page operations. Bound
the entire escaped JSON envelope to8KiB for a read/page and4KiB for root, including
IDs, clocks, coverage, omissions and errors. Limit UTF8 request bytes and argument
lengths before allocation/parse. Cross-store IDs and stale/malformed continuation
tokens refuse; bind requests/locators and continuations to expected project/store
UUIDs, since opaque entry IDs can coincide across stores. No old-page cache bypasses Forget. Root retains its distinct
authority/category/scoped quarantine rules in root_projection.rs, applied before
counts/limits. A model fragment requires the repository's ContextualUserFragment
contract and separate manual review if any new item can exceed1k tokens. CLI JSON
is not by itself authorization to inject arbitrary model context.

## Acceptance and review units

Keep each stage below500 complex changed lines; split extraction from behavior
where needed. Do not add a public unchecked pool constructor or depend on core.
Domain dependency additions require both Cargo and Bazel lock maintenance; new
compile-time SQL/file inputs require Bazel data declarations. No duplication of
legacy SQLx history into the kernel ledger. Each source slice gets Windows/Linux
targeted just tests, scoped fix/final fmt, fresh exact Droid review and a Luna
scratch launcher journey after building a frozen local helper under shared gates.

Effective fixtures must compare whole native/neutral responses for live, retired,
superseded user memory, unapplied/applied proposals, source exclusion, incomplete
coverage, malformed/oversized context, foreign IDs and relations/premises. Include
Forget between reads, stable within-read snapshot, escaping expansion, exact byte
caps, precision above2^53 and move/reopen identity. Tests of source query parity
do not substitute for runtime receipts. Stage1 alone grants no effective-read
claim; stages2/3 grant no M0/S2/S3/native/MCP/offline/HAL/customer acceptance.
