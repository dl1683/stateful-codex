# Stateful Codex decisions

Owner-approved consolidation direction, **2026-10-20**. Runtime source baseline
`997007527c5bb1d03c4d6b67cd5f8786b37b3e17`.
The [roadmap](./STATEFUL_CODEX_ROADMAP.md) is the complete inventory;
[consolidation](./STATEFUL_CODEX_CONSOLIDATION.md) supplies recovery refs.

## Archive preserves work without delivering it

Only selected small, current-API ports enter `main`: operation-specific hold
accounting, gateway security and direct browser memory controls/drafts/paging.
Whole historical branches contain stale APIs, rejected predecessors and
unqualified startup/migration behavior. Merging them behind disabled flags
would retain compile, startup, migration and maintenance exposure.

Therefore held/failing compaction and other unfinished work are archived as
annotated tags plus disposition notes, **not merged behind a flag**.
An archive is a complete preservation disposition, not implementation or release
acceptance. Reuse requires a small reviewed port onto current `main`.
Item6's indexing/attribution residual remains pending the owner's requested
short review; neither integration nor rejection is invented here.

## Compaction remains held

The archived active-compaction stack at
`e1db7324a6864d099ceee8261d0276a09788ba8b` includes a dependency graph, not a
standalone qualified fix. Retained evidence records:

| Recorded check | Disposition |
| --- | --- |
| `codex-cli` normal suite | FAIL |
| `codex-app-server` normal suite | FAIL |
| `codex-stateful-extension` normal suite | FAIL |
| `codex-core` normal suite | FAIL |
| Earlier broader-memory check | FAIL, separate retained evidence |
| Original installed-native controller | **23 total / 8 PASS / 15 ERROR / exit 1** |
| Later source approval | Narrow source approval only; **RELEASE NO** retained |

These are **historical recorded results**, not suites run by this documentation
author. The landed [evaluation accounting](./STATEFUL_CODEX_EVALUATION.md)
retains operation-specific holds. Packet persistence, model-visible root,
exact wording survival, current source truth, cold restoration and launcher
fidelity are separate properties. No one pass substitutes for the others.

## Rejected mechanisms and held alternatives

- **Rejected:** regex confirmation as user authorization. Archive
  `archive/20261020/heads/h1done/item5` preserves the rejected approach, not a
  feature to restore.
- **Rejected:** treating source/assistant text, a quotation, confidence or a
  route match as a binding human rule. Preserve explicit provenance/authority
  and deliberate Apply/direct controls.
- **Rejected:** a foreign agent borrowing human direct-add/correct authority or
  unrestricted Host updates. Generic blackboard upsert is not an approved
  foreign writer. Foreign writes and live upgrades remain held.
- **Held:** exact-input correction repair, provenance/selected-checkpoint
  restoration and exact history/E2. No new automatic quotation/model-transcript
  route follows from their archived designs.
- **Inactive:** canonical single-file/domain machinery and capsule experiments.
  Do not conflate library availability with native cutover.
- **Rejected for this consolidation:** importing old knowledge or restarting the
  retired one-file rebuild. Retain project-local native placement/schema/locks.

## Branch, upstream and custody policy

**One maintained branch: `main`.** Temporary external PR heads are review
vehicles, not maintained product lines. `archive/20261020/...` refs are
historical tags. Archive tag publication requires its own privacy audit and
explicit authority; local presence is not public availability.

Pinned upstream batches are reviewed and tested before entering `main`.
Checked/fetched/merged/validated/promoted are different statuses. Archived B0
work and migration 59/60 compatibility do not authorize an upstream merge.
No Stateful contributor is directed to submit fork-specific work to OpenAI.

One executor owns each scope; coordinate one build-producing command
machine-wide. Use fresh cross-engine review, scoped tests, one Cargo job,
RAM admission and separate full-suite approval. See [contributing](./CONTRIBUTING.md).

The legacy common Git object host remains an **inactive protected dependency**
of another build checkout. Consolidation does not authorize moving or deleting
that host or protected checkouts. This documentation pass does not touch them.
Private uncommitted payload preservation is separate from commit tags: an
archive tip does not contain dirty files.

## What this does not decide

No deletion, source-worktree cleanup, push, Dependabot PR closure or repository
setting change is performed by these docs. The approved proposal to stop
Dependabot **version-update** PRs is not evidence it was executed, and must not
silently disable security updates. Public archive safety, remote-main alignment,
recovery custody and independent cold-follow acceptance remain separate gates.
