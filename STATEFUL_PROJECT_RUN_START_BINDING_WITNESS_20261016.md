# Persisted run-start binding witness - 2026-10-16

INACTIVE SOURCE ONLY. No native routing or terminal certificate is supplied.
Source919ded93450315a2e18805dd79289406065c25fc adds201 lines in4runtime files.
Merge05aeb4dcfbff042ab721b61d929d590f7abe030e takes approved maina118 without
runtime/dependency/root-manifest changes or conflicts; exact source/merge approved
in source_helper_review2.txt. Source/API review has no P1/P2 findings.

revalidate_project_run_start consumes an existing ProjectWriteTransaction, requires
the actual runtime manifest and selects one primary-key run. Canonical project
ownership must equal the guard identity; created_at_ms must be a nonnegative
SQLite INTEGER exactly equal to the caller expectation. Missing/foreign/malformed
or mismatched values refuse. Errors drop the same guard, queue rollback and make
its owner close-only; reconciliation awaits owner close. Success returns that
same guard, with no write/commit shortcut, pool, corpus scan or filesystem access.

The trusted caller MUST derive expected_created_at_ms from opaque PI preparation,
validate its clocks under the SAME guard, and complete the SAME run with that
guard. Existing acceptance/revision/child/source/steering checks remain necessary.
Supported writers preserve created_at_ms; arbitrary SQL tampering is unsupported.
This scalar precondition alone is neither authentication nor a whole certificate.

Actual local just test runtime suites: Windows80PASS0skip11.172s; WSL80PASS0skip
8.300s. Five new tests cover zero/>2^53, cross-domain writes, mismatch/negative,
missing/foreign, malformed TEXT/REAL without coercion and later stale acceptance.
Full9579-leaf source maps stable for each suite. Scoped fix/final fmt exit0; no
postcleanup retests. The later approved merge preserves runtime/dependency trees.
No dependency/schema/config/core/context or launcher changes.

External helper uses both compiled domain migrations in one fresh project SQLite
store. It stages a marker, composes opaque PI preparation -> persisted run-start
binding -> PI clocks under one guard, and commits only the marker on acceptance.
It NEVER calls terminal completion and always returns completionCertified:false,
syntheticOnly:true,nativeAttached:false. First controller review required the zero
boundary; corrected helper match expected/stored0, other six fixtures unchanged.
Historical helperbuild1 remains retained; revised locked/offline build2 passes
6.39s, two jobs, shared mutex and RAM/process admission. Root sources unchanged.

Fresh experiment_review2 approves corrected direct/native preparation; direct1
PASS_SYNTHETIC_COMPONENTS_ONLY for7cases: zero match, mismatch, >2^53 match, REAL
refusal, missing/foreign run and late agent knowledge. Independent full67table/
204schema-object/35migration-row baseline and all before/after/live images match.
Success changes only the marker, all refusals preserve the full image. Exact FTS
transaction boundaries/closed immutable reads/no sidecars; every input/source and
artifact postflight passes, root exit0 and active BEFORE/after cleanup0.
Fresh frozen_luna1_review APPROVE checks102bindings, complete9583file frame/archive
and all direct7case images/25inputs. Manifest3ded6b13ee61ebbb129b51e5390e38615acfe051afe1ca6c0a6b9d15dc6085a1.
ONE actual unchanged scodex.cmd/gpt-6-luna trial runs15.252s with3responses,
exactly2sequential custom calls, no other tools and no compaction. Mismatch refuses
and rolls back; large retains exact expectedCreatedAtMs STRING9007199254740993.
Strict complete raw/public helper replies and final/-o expected objects match.
PASS_LUNA_INACTIVE_COMPONENTS_ONLY; no judge, normalization or model retry.
Independent audit_trial1_closed.py/json rehashes all102pins and current9583source
leaves, replays strict raw/public payload/final and immutable complete artifact
checks, verifies raw rollout hash without copying it, and checks rootExit0,
activeBefore/after0. All PASS. It is closed before root documentation changes.
Native ordinary engine-home session writes may occur; memory remains unattached.

The separate earlier PI/stream Luna trial remains FAIL_FINAL_OUTPUT_ENVELOPE_RETAINED;
this unit changes source and tests its different runtime precondition. Its explicit
stdout-parsing prompt cannot regrade that earlier ambiguous final-output failure.
No failed evidence is overwritten or upgraded, no judge/model retry/normalization.

First project-owned company-brain milestone remains NOT FULFILLED. Native engine
routing/populated home migration/old-writer retirement and complete atomic completion
certificate/fence removal remain open. Actual Forget/compaction survival/MCP readers,
native Unicode/offline/HAL/customer use cases remain unqualified. No native install,
legacy-home conversion, account reset or source-policy weakening was performed.
Protected dirty upstream B0 worktree and historical refs are preserved; only stateful
remote is eligible. Exact external receipts remain under:
C:/Users/devan/sc_dogfood/lsr_drivers/reports/stateful_rebuild/run_binding_20261016
Closed source frames/ZIPs become HISTORICAL after documentation/promotion; do not
replay sealed controllers or claim their old complete membership is still current.
