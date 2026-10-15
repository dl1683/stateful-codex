# HAL child-shell UTF-8 diagnostic - 2026-10-15

This is a synthetic child-shell diagnostic, not a native sandbox reproduction,
product patch, source-matched native build, translation result or HAL acceptance.
No repository code, native binary, sandbox policy or customer input was changed.
The original read-only CLM question-mark failure remains FAILED.

Source prefix was extracted from pinned Git922ee45099e0650cf97f87de106920cc50da2794,
shell-command/src/powershell.rs (source SHA25653f8ec437d71d863bb220a504efa68d687a63d841ffbdab73b834cb0709a3820).
It puts the Console.OutputEncoding setter and three file-reading defaults in one
try block with an empty catch. The unified-exec callsite is
core/src/tools/runtimes/unified_exec.rs:594-599. Source inspection also finds
Windows sandbox shell fallback selection; it does not prove which executable or
prefix produced the separately installed native baseline's failure.

A new scratch copy of the same405-byte synthetic fixture matched SHA256
c4f90eae982f01c0c9308a99ac84c06545b1fa3e5e75e86b47c3620d48de5120.
The child scripts restrict only their own SessionState.LanguageMode. Native
sandbox settings are untouched. Powershell5.1 and pwsh7.6 used -NoProfile;
stdout/stderr were captured as bytes. All18 children exited and were collected.
Ten read cases, two initial diagnostic probes, and six follow-up cases are
separate records; these are not18 passing tests or independent native replications.

| Read condition | Windows PowerShell5.1 | pwsh7.6 |
|---|---|---|
| FullLanguage, default read |529bytes, decoded JSON wrong |407bytes, exact source plus CRLF |
| FullLanguage, source prefix |407bytes, exact source plus CRLF |407bytes, exact source plus CRLF |
| ConstrainedLanguage, default read |529bytes, decoded JSON wrong |407bytes, exact source plus CRLF |
| ConstrainedLanguage, source prefix |529bytes, decoded JSON wrong |407bytes, exact source plus CRLF |
| ConstrainedLanguage, prefix then explicit file UTF-8 |407bytes, exact source plus CRLF |407bytes, exact source plus CRLF |
| ConstrainedLanguage, separate file-default diagnostic |407bytes, exact source plus CRLF |407bytes, exact source plus CRLF |

The corrected property-only probes report Console.OutputEncoding.CodePage65001 in
both modes. In FullLanguage, the source prefix leaves default=utf8/errors=0.
In ConstrainedLanguage, it leaves an empty Get-Content default and records
PropertySetterNotSupportedInConstrainedLanguage. This isolates skipped file-reader
configuration in this controlled process. PowerShell7.6's read stays correct here
because its default handles the fixture. No child read produced the native
329-byte ASCII-only question-mark result; inherited console conditions differ.

The first ContainsKey diagnostic probes themselves failed with the expected CLM
method-invocation restriction. Those failures are retained. The follow-up probes
use property/index reads and do not use that prohibited method. A diagnostic
script setting a file-reading default separately is not a proposed or qualified
product repair; it leaves the console restriction intact and says nothing about
native sandbox output encoding. Do not turn it into H1 acceptance.

Evidence root:
C:/Users/devan/sc_dogfood/lsr_drivers/reports/stateful_rebuild/
Scripts: h1_child_shell_diagnostic.py, h1_child_shell_probe_v2.py.
Directory h1_child_shell_diagnostic/: receipt.json, probe_v2_receipt.json,
powershell_source.txt, exact fixture and each script/raw stdout/raw stderr.
The existing native trial/source-match limits remain in
STATEFUL_HAL_PRESERVATION_BASELINE_20261015.md.

Next H1 qualification must bind actual installed/candidate source and companions,
resolved sandbox shell executable and child console encoding. Test file decoding
and stdout transport separately through the unchanged read-only policy, then
compare exact source/tool/final bytes. Preserve image/non-Git behavior and the
failure receipts; no full-access substitution, ASCII fixture replacement, error
suppression or weakened checker can qualify that gate.
