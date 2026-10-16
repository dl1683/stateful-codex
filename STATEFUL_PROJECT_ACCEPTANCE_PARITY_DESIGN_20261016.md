# S2 acceptance parity before project-store activation

Design only, based on main `3b005a5e68e720b3b3e903b521d8886c76ceadb8`.
No new completion path, policy, native routing or migration is activated here.
The canonical order remains S1 -> M0-M3 -> S2 -> S3. The parallel upstream
executor retains B0 custody; its unqualified dirty branch must not be imported.
The owner's first project-owned database milestone is still unfulfilled.

## Current paths and the gap

`ext/stateful/src/tools/run.rs` acquires the legacy intelligence completion fence,
prepares knowledge, calls `acceptance_decision`, then commits through the separate
home-scoped runtime. `tools/run_acceptance.rs` settles pending commands for a
bounded interval, leases verification, proposes uncovered requirements, reads the
ledger and source files, and constructs `AcceptanceCommit`. For a nonlocal
executor it refuses host-shadow file evidence. It also checks applied steering
and discloses optional unverified/manual/dismissed criteria in the result.

`stateful-runtime/src/acceptance_storage.rs::enforce_acceptance_gate` re-derives
durable policy in the terminal transaction: ledger revision and workspace
generation, pending commands, current lease, latest obligation and blockers,
applied steering, criteria/dependencies and uncovered request sentences. The
lease consumption rolls back with a failed transaction. Do not replace this
policy with a caller-supplied Boolean or an apparent successful checker exit.

`stateful-runtime/src/project_completion.rs::commit_project_run` already delegates
to the same transaction body and rejects foreign project-tagged dependents. Its
public inputs are explicitly trusted; an `AcceptanceCommit` is not an opaque
certificate that a real command or source observation occurred. The existing
extension composition fixture uses synthetic acceptance and proves transaction
composition and rollback only, not host acceptance admission.

`project-intelligence/.../project_write.rs` revalidates a privately prepared
negative-learning result using knowledge/eligibility clocks and coverage under
the writer. The caller must bind its timestamp to the persisted run, validate
material findings/source bindings separately and perform no invalidating writes
after that check. The positive durable-learning disposition is a distinct
remaining obligation; negative-learning coverage cannot stand in for it.

## Preparation outside the writer

The extension, not core and not the neutral CLI, owns a private prepared
completion value. Its constructor must retain the existing host policy rather
than accept model-supplied observation maps. Bind one project/store identity,
run ID/start/revision, knowledge/eligibility snapshot, lease owner/attempt,
ledger revision/generation, latest obligation sequence and executor identity.
Carry bounded checked findings/source observations, result and acceptance
disclosures. Use existing hard payload caps and refuse incomplete coverage.
Do not inject this internal structure into model context.

Pending-command settling, requirement-proposal work, filesystem traversal,
hashing, evidence/premise observation, result rendering and any lease acquisition
must finish before the final writer is acquired. Preparation may use several
short transactions, but every dependency it relies on needs a final check.
All preparation reads and lease operations must target the SAME project store;
retaining the old home runtime as a preparation side channel is not shared-store
parity. No cross-database fence or commit may be hidden in this phase.

Preserve the supported executor distinction: matching path strings do not make
the app-server host the remote executor. An unsupported observation stays
unavailable, yielding refusal or truthful Partial according to existing policy.
Read failures, omitted pages, incomplete eligibility and absent source bytes
cannot become successful save/completion receipts.

## Short final transaction

One private orchestration entry consumes the prepared value. Its sequence is:

1. Open the qualified existing project store with expected identities/domains.
2. Acquire one `ProjectWriteTransaction` and validate its store/domain admission.
3. Revalidate the same run/start/revision and every knowledge/eligibility/source
   database binding used by preparation. Validate positive learning separately
   when selected. Do not reconstruct observations from model tool arguments.
4. Re-run the existing durable acceptance gate on this same connection: lease,
   ledger/generation, pending commands, latest obligation/blockers, steering,
   coverage and criterion/dependency verdicts. Bind the pre-rendered disclosure
   to these unchanged verdicts; a mismatch refuses rather than removing caveats.
5. Write terminal result and final obligation atomically. Place the final
   knowledge check immediately before terminal commit, without an intervening
   domain write that can invalidate it. Commit once through the guard.

No await of a command, background scan, other database or filesystem observation
belongs inside steps 2-5. Database queries must have measured bounds; moving a
full history scan under the writer to obtain apparent atomicity is not accepted.
Do not remove the legacy fence or route native callers until both dispositions,
populated conversion and old-writer exclusion are qualified together.

## Source freshness and the supported guarantee

SQLite serializes database writers, not filesystem edits. A hash read before
acquisition establishes bytes at that observation, not unchanging bytes through
COMMIT. Neither the legacy fence nor the inert project guard freezes files.
Keep source observation timestamps/provenance and existing mutation generations
honest. Supported application mutations must be durably accounted before
completion, with admission preventing an in-flight application command from
being mistaken for a quiescent observation. Queue/drain admission is a separate
witness, not implied by this writer API.

An independent external editor or same-user arbitrary SQL rewrite is not made
safe by a clock equality check. Do not claim universal present-time filesystem
certification or silent source stability. Qualification must name this boundary
and reproduce refusal when a supported observed mutation intervenes. Any stronger
fresh-close/immutable-Work certificate belongs to the separately reviewed S8
contract, not an early substitute for S2 acceptance parity.

## Cancellation, acknowledgement and replay

Before COMMIT, refusal/cancellation drops the consuming guard and makes its
writer close-only. Await owner close before inspecting durable state or opening
another writer. A lease committed during preparation can remain until explicit
release/expiry; cancellation must not consume another owner's lease.

Cancellation or IO failure DURING COMMIT has uncertain outcome. Close first,
then read run/result, final obligation and the operation's durable identity from
the same project store. A matching terminal outcome returns the prior receipt;
an unchanged running outcome may prepare afresh. A different terminal result,
changed retirement or ambiguous evidence refuses. Never infer rollback from a
lost acknowledgement, generate a second obligation or replay stale eligibility.
The existing composition tests do not yet prove this recovery contract.

## Next reviewable implementation and acceptance

First extract connection-based runtime read/lease operations without changing
native routing; share their legacy policy bodies and prove same-store binding.
Then implement private preparation/composition in separate small modules. Keep
complex review units below 500 changed lines; do not activate half-routed native
code to satisfy that budget. Do not introduce test-only public crate helpers.

Required major behavior witnesses before native activation:

- A real host-admitted command with source observations completes through the
  shared writer; exact terminal result, disclosures and final obligation agree.
- Checker/artifact edit, latest failure, pending or unaccounted command,
  uncovered requirement, unreconciled steering and changed obligation refuse.
- Forget, knowledge/source-generation change, wrong run/start/store/project and
  unsupported remote-host observation refuse without durable partial mutation.
- Failure after lease consumption rolls back all affected tables; cancellation
  before/during COMMIT and lost-ack replay have accounted whole outcomes.
- Both learning dispositions preserve existing policy and useful Answered/
  Partial endings; no model-invented acceptance or checker replacement passes.
- Concurrent first opens/writers/indexing, completion versus Forget and another
  project satisfy bounded contention without holding a writer over a slow scan.

Use auto-environment public core/app-server integration tests for new agent
logic/native routing, alongside domain transaction tests. Keep current extension
baseline repairs distinct from this future coverage. A real launcher migration
journey must observe one `.stateful/memory.sqlite`, retain populated facts,
exclude old writers and verify correction/Forget plus neutral reads. No such
native milestone is established by this document or by inactive fixtures.
