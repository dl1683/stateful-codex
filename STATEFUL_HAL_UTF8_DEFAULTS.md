# H1: preserve file-decoding defaults when console setup is refused

The retained HAL baseline failed exact Marathi JSON under native read-only
execution. Separate child-shell controls showed that the single best-effort
PowerShell try block abandons all file-decoding defaults when Constrained
Language Mode refuses its first console-encoding setter. This is one observed
defect; the native question-mark failure has not yet been reproduced or fixed.

Split console setup and file-decoding defaults into independent best-effort
blocks in shell-command's existing PowerShell prefix. Keep the same UTF8
defaults for Get-Content, Select-String and Import-Csv; explicit command encoding
continues to win, and writing cmdlets keep their existing defaults. Do not change
sandbox policy, approval checks, model instructions or source/checker bytes.

Public shell-command integration tests will launch Windows PowerShell5.1,
establish UTF8 console output before entering CLM, then use the production prefix
to read a BOM-less Unicode fixture with implicit file encoding. Compare complete
output objects/bytes. This proves file-decoder independence while console setup
is refused, not the actual native sandbox transport. Non-Windows compilation
retains ordinary crate coverage; Windows-specific execution is labelled.

Qualify the actual native route separately with source/binary/companion hashes,
the installed scodex launcher, Luna, the retained exact405-byte JSON and read-only
policy. Preserve all failed attempts. A child-shell pass cannot clear the native
329-byte failure. No translation, offline appliance or customer-delivery claim.
Local serialized builds/tests use jobs2 and the unchanged RAM floor; scoped fix,
final fmt and fresh exact-diff Droid review precede stateful-only promotion.
