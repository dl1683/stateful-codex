# Extension baseline gates repaired

Source `c945931e1ca8fec446a3b67546425ce981500911` changes only two test
fixtures. Fresh source review `gates_20261016/review1.txt` APPROVE; no product
acceptance logic, timeout, fan-in scale, native route or sandbox policy changed.
Evidence directory is
`C:/Users/devan/sc_dogfood/lsr_drivers/reports/stateful_rebuild/gates_20261016`.

The Windows fixture formerly denied listing the `.git` directory but left Git's
known child reads available. The correct inner repository could still be read.
Inheritable read denial now covers those children, retains the existing metadata/
directory preflight, checks HEAD read denial, restores permissions and requires
the unchanged Git-gap result. No production Git fallback was relaxed.

The 4096+64 predecessor fixture now uses two Tokio workers for SQLx handoffs.
Every public entry creation, model batch, two reopens, history bound and replay
refusal remains. The ordinary nextest 60-second deadline is unchanged.

## Recorded verification

- `windows1.log`: 338/338 PASS, zero skips/retries. Fan-in43.984s;
  Git-marker0.519s. Entire suite416.253s includes the unchanged long directory
  witness with its preexisting Windows timeout override.
- `linux1.log`: 339/339 PASS, zero skips/retries. Fan-in48.500s;
  Git-marker0.150s. Entire suite95.851s. Previous run's failures/timeouts remain
  retained in `terminal_20261016`; they were not regraded.
- `windows_witness1.log`: explicit success output confirms actual ACL application
  and restoration; no `not run (preflight)` return. One selected test PASS,
  337 filtered tests; this is not another full-suite run.
- These runs used the shared local build mutex, jobs2 and RAM floor23221225472.
  Windows explicitly cleaned/recompiled the extension to avoid the previous
  stale-artifact problem. No Rust process was killed and no cloud build ran.
- Full-repository manifests are retained. Two explanatory comment lines were
  added after the Linux run and while Windows dependencies compiled; the parity
  design document was added later. Do not claim all repository frames identical.
  The tested behavior is unchanged by those comments; source revisions and the
  original executed maps remain explicit. Cleanup/formatting is separately logged.

## Installed-launcher check

`luna_trial1/score.json`: one unchanged-launcher gpt-6-luna session,12.966s,
two completed command items, one completed turn. The non-Git scratch task copied
Marathi JSON byte-for-byte, verified hashes and returned the exact complete JSON.
Deterministic byte/JSON and launcher/binary/input binding checks passed.
Launcher SHA256 `daf2303629de12cd1a049c791a141f154edddfeca4463ce38a903ecec9ea8fae`;
installed binary `5e476700701b28394d319863498a6e6789a2f64317d9756b69d12be8818043c7`.
No candidate was installed; this checks the installed file workflow only, not the
candidate tests, native worker, read-only Unicode route, or offline acceptance.

## Storage boundary

`STATEFUL_PROJECT_ACCEPTANCE_PARITY_DESIGN_20261016.md` at `d2b599a82f` received
fresh DESIGN APPROVE (`parity_review1.txt`). It is design-only. Preparation and
hashing stay outside the writer; durable run/knowledge/eligibility/lease/ledger/
obligation/steering checks and terminal facts share one final transaction.
The review also preserves submitted/acknowledged steering gates and identifies
unbounded accumulated acceptance-request inputs as a remaining qualification risk.
Durable replay identity, positive learning, real host admission, cancellation,
source observation, migration, routing and old-writer exclusion stay open.

These repairs unblock the extension baseline; they do not fulfill the owner's
first project-owned database milestone. B0 remains unqualified and parallel-owned.
