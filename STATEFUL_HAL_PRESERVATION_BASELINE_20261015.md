# HAL preservation baseline: 2026-10-15

> **Archived historical record, not current operating instructions.**
> Use [current state](./STATEFUL_CODEX_CURRENT_STATE.md) for status and holds.
> Historical external evidence is not distributed in this repository.
> Paths and personal identifiers are redacted; original bytes and any
> historical digest claims refer to the version preserved in Git history.

H1 preparation alongside project-store foundations. This is installed-baseline
evidence, not a candidate gate, translation judgment, IndicTrans2 invocation,
customer delivery, offline qualification, or a completed HAL vertical slice.
The project-local ownership milestone remains gated on registry/opening, neutral
reads, unified transactions and verified native migration.

## Frozen inputs and execution

External evidence: `sc_dogfood/lsr_drivers/reports/stateful_rebuild_h1_20261015/`.
The synthetic JSON contains a block ID, source/translation strings, Marathi
characters/digits, escaped newline and Windows path, unresolved `200[?]`,
`reviewRequired: true` and `receipt: IT2_NOT_RUN`. Its 405 UTF-8 bytes include
114 non-ASCII bytes; SHA256 is
`c4f90eae982f01c0c9308a99ac84c06545b1fa3e5e75e86b47c3620d48de5120`.
Both project copies have those exact bytes. Input hashes were rechecked after
all journeys. The image is a synthetic ASCII label with value `770.23`.
No customer document, translation engine or cloud host was used.

The actual `scodex.cmd` launcher used its installed 0.159.2 executable and
companion, identified in the S0 manifest. Model: Luna, high reasoning,
collaborative mode, non-Git scratch folders. Installed source matching is not
proved. All commands, structured events, sessions and final output files remain
in the external evidence root. The scorer is deterministic Python raw-byte
comparison plus JSON deep equality; no LLM quality judge was used.

## Observations

| Journey | Sandbox | Result |
|---|---|---|
| Exact JSON copy, first plain prompt | read-only | FAIL: final329bytes, zero non-ASCII; Marathi became question marks |
| Same prompt, independent folder with identical input bytes | read-only | FAIL: identical329byte output |
| Diagnostic prompt explicitly requesting console UTF-8 before read | read-only | FAIL: same output; console setter refused in Constrained Language Mode |
| Original prompt, diagnostic profile control | danger-full-access | PASS: exact405bytes and deep-equal JSON |
| Image numeric read, no tools | read-only | PASS: exact six-byte `770.23` |

All five exec invocations exited0. The read-only failures are not cleared by the
full-access control; these sandbox profiles are not interchangeable acceptance.
The explicit-encoding diagnostic used a changed prompt, not a third matched
replication. The full-access control's model command used a UTF-8 PowerShell
read; command-level details are retained in `preservation_results.json`.

The model-visible PowerShell command output already contains question marks in
the failing journeys; the model copies those corrupted strings into its final.
The full-access case preserves every JSON value, Marathi byte, escaped newline,
path, uncertainty marker, review flag and IT2_NOT_RUN marker. These are synthetic
preservation observations, not a claim that the translation itself is correct.

Direct unsandboxed subprocess controls of pwsh Get-Content, with and without an
explicit console encoding, preserve all114 non-ASCII bytes (plus a CRLF emitted
by Get-Content). The restricted diagnostic logs
`PropertySetterNotSupportedInConstrainedLanguage`, including a nested PowerShell
attempt. Restricted console encoding is therefore a plausible contributor,
not a proved complete cause or an identified defect in a newly built candidate.
No policy was weakened to turn the failing read-only gate into a pass.

## Corrected diagnosis and next qualification

The initial operator diagnosis blamed fixture creation. Independent byte hashes
and codepoint inspection disproved that: the source files were valid and
identical all along. `v0_INVALID.md` retains an explicit correction; both the
original journeys and all later controls remain available. Do not classify the
first two observations as invalid input trials or omit their failures.

Source inspection finds the best-effort UTF-8 prefix in
`shell-command/src/powershell.rs`, its unified-exec callsite, and raw final-file
writing in `exec/src/event_processor.rs`. This is static source context; it does
not bind the installed binary to that source. A remedy must preserve read-only
policy and qualify the actual Windows tool route with matched source/binary
identity, a public integration test and exact-byte output before promotion.
Do not suppress CLM errors, replace source with escaped ASCII, change a required
checker, or substitute the full-access control as acceptance.

H1 must additionally qualify exact tool strings and delivery through interruption,
compaction and restart, JSON/image combinations and real adapter provenance.
CIDCO's human flags/delivery review, eligible offline inference, later specialist
modules and capacity acceptance remain separately open. This baseline supplies a
concrete failure fixture and protected image smoke for the ongoing rebuild.
