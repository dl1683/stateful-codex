# Inactive console worker response ordering

Source `657d5285d1` adds a fixed-size parent-side sequence to the leaf protocol
crate. Ready -> Spawned -> RootExited assertions must match the parent's nonce
and actual worker PID. Wrong/zero/self PIDs, duplicates/reordering and nonce
mismatch refuse permanently. Reported failure still refuses AFTER root
observation. All u32 exit/error bits survive; error text is bounded and static.
RootObserved grants no terminal, preservation, drain or completion authority.

There are no handles, OS calls, process ownership, jobs, clocks, buffered history,
authentication, local lifecycle actions, native route or context injection.
The frozen transport design's complete paired-job arbiter remains future work.
Raw transport decoding remains a separate prerequisite of sequence observation.

Fresh source Droid APPROVE. Windows/WSL protocol 24/24 PASS each, zero skips/
retries. All 9,600 executed leaves verify against retained `657d` Git objects;
locked/offline helper build has identical complete source/auxiliary frames.
Local shared mutex, jobs=2, RAM floor 23,221,225,472 bytes; no cloud build.

Independent complete typed oracles pass 13 decoded-response cases and a static
nonzero IO refusal. Two ordinary and 11 refusal sequences cover full u32 exit,
duplicates/reordering/sticky failures, foreign worker/self target, malformed
frame/nonce, zero constructor and reported failure after root observation.
Trial and isolated helper-home inventories detect writes; this is not OS
confinement. Bound successful direct exit/stdout/stderr/report gate Luna.

The controller's literal shell prefix was corrected before any HAL session,
using the retained failed acceptance session's JSON-rendered shell reference.
The reference, exact known executable path, finite new-command allowlist and
synthetic mutation controls are hashbound. Extra statements, encoded commands
and altered bodies refuse. No past actual score was regraded.
Fresh helper/controller reviews approve their respective component scopes.

ONE actual unchanged-launcher gpt-6-luna session PASS: 12.150 seconds, one
inspector command, exact whole tool/final/report/case outputs and 44 archived
bindings. This is the `657d` external helper, not a later cleanup/native binary.
The existing native 405-to-329 Marathi failure remains OPEN.

Scoped fix and final fmt PASS; fix made no source edits. No Rust retest followed
cleanup. The conflict-free merge with approved main preserves exact protocol,
project-store and runtime component trees (`merge_source_identity.json`);
it does not qualify combined native behavior. Exact closeout/promotion receipts
separately determine landing. Protected dirty upstream B0 remains unqualified.

Authentication, restricted launch, paired-job races, expected worker exit/EOF,
pipe/drain/live-child accounting, companion packaging, native image/Marathi,
offline inference and customer acceptance remain open. The first project-owned
database/import/old-writer milestone remains unfulfilled. Evidence lives under
`sc_dogfood/lsr_drivers/reports/stateful_rebuild/sequence_20261017`.
