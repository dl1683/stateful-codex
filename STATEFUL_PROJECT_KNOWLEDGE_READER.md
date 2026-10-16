# S1 intelligence snapshot admission

This unit introduces ProjectKnowledgeReader, consuming a ProjectStoreReader only
through the intelligence crate's actual compiled manifest check. Its inspect
operation requires both expected UUIDs, revalidates the kernel snapshot, checks
typed clocks/current revision pointers, the fixed complete direct-owner inventory,
declared and missing semantic child references, and hierarchy cycles before
returning metadata/coverage. Ordinary refusals finish the snapshot before reuse;
kernel cancellation/commit refusal remains close-only. Explicit close awaits release.

Canonical kernel project UUID is the fresh-domain owner. Foreign/legacy IDs refuse;
there is no conversion, first-row selection, repair, pool or home fallback. Only
intelligence ownership is certified: this is not runtime ownership or native
attachment. No entry content, model context, effective verification, root, CLI or
new writer is exposed. This stages the ownership prerequisite of the separately
approved attributed-content contract; the scalar loader remains the next unit.

Tests use the real compiled intelligence schema in fresh project-local stores.
Windows/local WSL scoped tests, Cargo/Bazel lock maintenance, scoped fix/final fmt,
fresh exact Droid review and a separate local frozen-helper/Luna witness are gates.
No first-milestone, migration/atomic-completion, native Unicode, MCP/offline/HAL or
customer qualification follows from metadata admission.
