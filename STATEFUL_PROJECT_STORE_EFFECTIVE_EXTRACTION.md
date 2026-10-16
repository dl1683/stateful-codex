# S1 effective exact-read extraction

This behavior-preserving source slice factors the existing `get_hit` transaction
body into private `effective_hit::read_on`. Native `get_hit` still owns begin and
commit. The shared operation uses the same ModelTool eligibility and full hit
loader, including existing relation/premise behavior; incomplete coverage remains
`None`. It opens no pool, home, path, or transaction and grants no caller-selected
authority. Future admitted domain callers must keep one read snapshot throughout.

This is the extraction prerequisite in the approved staged effective-read design,
not the bounded neutral facade. It does not expose new public API or change agent
behavior. Nested allocation/eligibility, explicit coverage metadata, canonical
project ownership and bounded escaped envelopes require the next separate freeze.
Native migration, completion/Forget atomicity and the first milestone stay open.

Validation plan: full scoped intelligence tests on Windows and local WSL, including
new whole-hit snapshot/retirement and coverage/project-miss regression fixtures;
scoped fix then final fmt; fresh exact-diff Droid review before integration/push.
Existing tests retain applied/unapplied proposal, exclusions, user supersession,
relations and premise coverage. No new user command exists in this slice; a Luna
launcher check can exercise only existing native memory behavior, not neutral reads.
