# Glossary

See [architecture](./ARCHITECTURE.md) for source links and data flow.

| Term | Meaning |
| --- | --- |
| Project | Selected durable scope with roots and an identity; not merely the current chat |
| Hierarchy | Project → directory → file → anchored region structure |
| Anchor | Bounded scheme/locator inside a file; not proof that the bytes are unchanged |
| Blackboard | Retained revisioned knowledge/claims/rules, including provenance and lifecycle |
| Context map | Navigation metadata pointing to evidence, not an independent truth store |
| Root blackboard | Bounded project-level projection considered for fresh model context |
| World State | Host-composed, bounded current context with snapshot/delta semantics |
| Fingerprint | Identity of source bytes, not a verdict that a conclusion is current |
| Provenance | Recorded origin of text; distinct from permission and verification |
| Authority | Why an entry may guide behavior, such as human direct versus assistant reported |
| Verification | Recorded support/dispute/freshness, not confidence or authentication |
| Scope | Where knowledge is eligible; historical investigation scopes remain unsupported/held |
| Revision / CAS | Version and compare-and-swap precondition preventing stale mutation |
| Action identity | Client key binding an explicit add action to its payload/retry |
| Receipt | What a committed memory action/capture actually retained, proposed or omitted |
| Apply | Deliberate promotion of supported pending words; not an inference from a quotation |
| Correct | Revision-pinned replacement producing a successor, not silent history editing |
| Forget | Retirement from eligible effective reads, not physical erasure of history |
| Undo | Receipt-specific reversal, not a general database rollback |
| Native home | Ordinary Codex configuration/state layout with several SQLite files and sessions |
| Per-project store root | Placement of that native home under the project or an explicit override |
| Canonical project store | Separate domain/identity-managed `memory.sqlite` components, inactive for native conversion |
| Neutral reader | Bounded read-only inspection without an engine login; native and canonical modes differ |
| Ask | Answer using project context without starting a Stateful run |
| Work | Run workflow in autonomous, collaborative or socratic mode |
| Answered | Terminal unverified answer, not acceptance-certified Completed |
| Acceptance ledger | Host/runtime accounting for prescribed checks and completion obligations |
| Compaction | Replacement of long active conversation context by a smaller checkpoint |
| Conversation packet | Host-retained selected whole-message records with bounded coverage |
| Checkpoint | Persisted conversation boundary, or a separate volatile Work nudge; specify which |
| E2 / exact history | Paused design for exact eligible historical access, not a qualified live route |
| B0 | Archived inherited accounting/recovery work preceding upstream integration batches |
| HAL | Customer application/platform acceptance track, not established by component tests |
| LSR | Separate latent/routing research slice; not production learning or truth authority |
| Landed / implemented | Present in source, not a blanket reliability or platform qualification |
| Inactive | Component exists but the relevant native integration is not activated |
| Held | Execution, activation or acceptance still blocked/unqualified |
| Rejected | Mechanism deliberately not adopted; archival does not revive it |
| Archived | Historical recovery disposition, not feature delivery |
| UNVERIFIED | No successful execution evidence for the named command/platform/scope in this pass |
