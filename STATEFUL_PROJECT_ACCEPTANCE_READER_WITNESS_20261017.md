# Inactive project acceptance snapshot reader

Source units: `15b0024a6c` (snapshot domain), `6bae03f2f8` (reader),
`2a30378852` (static fixture SQL), `156cc64284` (sort key/caps/legacy witnesses),
`a8835976e6` (explicit deep equality). Native routing remains unchanged.

`read_project_acceptance_ledger` consumes one snapshot, requires the actual
compiled runtime domain and exact admitted project/run binding, then uses the
existing ledger decoder. Success finishes the snapshot; refusal/cancellation
leaves its owner close-only until awaited close. No ledger/request materialization,
knowledge authority, checker observation or real acceptance is established.

Complete-read budgets: one ledger, 32 criteria, 128 evidence-history rows,
64 reconciliations, 4,096 pending commands and 1 MiB aggregate stored fields,
including numeric/sort keys. These are private storage budgets, not context or
neutral API page budgets. Valid legacy excess explicitly refuses, including
large requirements and 65 reconciliations. The legacy reader's silent 64-row
cut is not copied. Byte accounting includes embedded NULs.

Windows/WSL project-store 52/52 and runtime 95/95 PASS, zero skips/retries.
Runtime/helper complete 9,601-file frames match; all executed leaves verify
against retained Git objects at `a8835976e6`. Store gates had unrelated runtime
preparation changes; their complete project-store inputs stayed identical.
The RED 94-test run exposed the oversized hidden sort key and an incorrectly
uncovered steering fixture. A real user criterion covers all applied scope
before 65 reconciliations; policy was not relaxed. Static-SQL/macro compile
errors and the PowerShell 5 gate failure remain retained.

Helper build1 is locked/offline/local, with a unique previously absent binary,
shared mutex, jobs=2 and RAM floor 23,221,225,472 bytes. The independent oracle
checks complete typed ledgers, schema/headers/all durable cells including
NUL/REAL/BLOB/sqlite_sequence, project files and isolated helper home, plus
malformed and valid foreign UUID refusals. This is not OS confinement.
Direct1 FAILED CP1252 stdout despite its saved PASS report; original inputs and
diagnostic remain. Direct2/direct3 passed; bound successful exit/stdout/stderr/
report now gate Luna. All 15 final controls pass; the earlier newline failure
remains failed. Extra-command/encoded-command/altered-body controls also pass.

Actual unchanged-launcher gpt-6-luna Luna1 remains FAIL_RETAINED: its shell
allowlist encoding differed despite matching data. All 31 inputs are archived.
Prospective corrected Luna2 PASS: 12.934 seconds, one inspector command,
34 archived bindings, exact whole tool/final JSON, 14 unchanged reads/eight
refusals/two distinct project stores. This qualifies the `a883` external helper,
not an installed native candidate or later cleanup binary. No regrading,
normalization or judge. Launcher and installed codex.exe remain unchanged.

Scoped project-store/runtime fix and final fmt PASS; fix made no source edits.
Formatting expands test layout, including one inherited verification assertion.
No Rust retest followed cleanup. Exact-candidate closeout and promotion receipts
separately determine landing. Evidence: `sc_dogfood/lsr_drivers/reports/`
`stateful_rebuild/acceptance_20261017`, including source comparisons, Git recovery,
raw failures, frozen sessions, controls and reviews.

First project-database milestone NOT FULFILLED: native attachments, populated
import/old-writer exclusion, real acceptance/source freshness, both learning
dispositions, Forget/compaction/MCP/offline/customer gates stay open. B0 remains
separately owned and unqualified. S1 -> M0-M3 -> S2 -> S3 remains controlling.
