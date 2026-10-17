# Canonical steering index — inactive S1 component

Executed source: `bd4128d04ccf4a54558cb7a21339baef442ad2d5`, based on
`c54831488aba4ac1e6cf6de41f7e910534edec35`. Canonical runtime appends project-only
version 14, a run/status/revision/time/id steering index. Native migration SQL,
openers and history remain unchanged. The explicit predecessor metadata accessor
supports complete-domain conversion; ordinary current reads never fall back.
Production discovery and foreign-dependent SQL keep their predicates and bindings.

Windows passes runtime100, CLI11 and extension338 tests; WSL passes100/11/339.
Actual SQLx query plans cover production statements, mixed statuses, foreign rows,
ties and recorded exclusion among 2,048 unrelated rows. Extension's existing deep
directory Windows test takes minutes; it passed without disabling or skipping it.
Scoped fixes and final fmt pass. No Rust tests follow cleanup; final formatting
is reviewed against the executed source and does not imply a rebuilt final tree.

Direct1 and original-launcher Luna1 pass the two-project transition matrix, with
67 tables per store. Typed whole-store comparisons include implicit rowids,
allocator high-water marks, sparse request ordinals, UTF-8 spans, identity,
retirement, history and large clocks. Only the new index and its history row may
change. Old/new CLI admission, malformed cohorts, wrong identity, replay, busy
cancellation, and uncertain COMMIT acknowledgement recovery are exercised.
The commit hook marks the injected point; fresh durable reconciliation establishes
the result. These are explicit component operations, not native-client cutover.

Luna1 passes in 27.728s with 286 bindings, one exact nested command, raw rollout
and model checks, persisted child outputs and isolated-home postflight. Immutable
Git objects prove all 9,619 executed and 9,616 predecessor source leaves. Local
builds use the shared mutex, jobs2 and the 23,221,225,472-byte RAM floor. Qualifying
Windows gates were repeated after auxiliary-input custody was strengthened.
Retained early helper lock/build and synthetic-control failures are not rescored.
Offline helper lock reconciliation removes unused dependency closure only.

Evidence: `sc_dogfood/lsr_drivers/reports/stateful_rebuild/canonical_index_20261017/RECORD.md`.
Fresh exact closeout and promotion receipts determine landing. Native activation,
shared-store import, old-client exclusion, lock cutover and the first database
milestone remain OPEN. Original launcher and installed native executable unchanged.
HAL's next creation-local parent-authority design is approved only as design;
there is no HAL runtime change, customer qualification or cloud admission.
