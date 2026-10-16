# S1 owned read snapshot

Implement only Stage1 of the effective-read design. Add an additive domain owner
retaining the compiled manifest and admission identity, plus a guard owning its
read transaction. Reuse exact domain admission and full schema/history/header/UUID
validation inside that same snapshot before exposing a trusted read connection.
Both UUIDs must equal admission. No pool, reopen, home, domain policy or native
activation is added; the raw trusted connection API remains compatible.

The owner becomes close-only before beginning async work. Successful explicit
finish awaits commit and makes it reusable. Failure, cancellation or unfinished
guard drop keeps it close-only; await explicit close before reopening/moving.
Trusted borrowers must not issue transaction control, pragmas or attachments.
No filesystem namespace or hostile same-user SQL security boundary is claimed.

New fixtures: stable snapshot across an independent WAL writer commit, next-read
visibility, readonly8, changed project UUID/store UUID/schema/history/header
between admission and begin, refusal without main-file repair, unfinished guard
refusal then explicit close/move/reselect preserving identity. Independent
connections are not independent processes or a killed-writer/power-loss test.
Drop proves the unfinished-guard path; async begin/finish cancellation is designed
fail-closed but not separately injected by these fixtures.

No dependency/lock/build-time input additions. Windows/Linux targeted just tests,
scoped fix/final fmt, fresh exact Droid review, and actual launcher/Luna witness
using an independently reviewed frozen external helper under the shared gates.
Effective knowledge/Forget policy, model context, native migration, atomic
completion, M0/S2/S3/MCP/offline/HAL/first-milestone acceptance remain open.
