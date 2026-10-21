# Stateful Codex current state

Updated **2026-10-20** for the standalone canonical `main`.
Runtime source baseline:
**`997007527c5bb1d03c4d6b67cd5f8786b37b3e17`**.
This documentation series changes no runtime behavior; later independent
P3b/P3c review fixes, if any, need their own source and verification pins.
Start with the [README reading map](./README.md#reading-map).

## What is current

- One maintained branch, `main`, in the canonical Stateful repository.
  Archive tags are historical, not development targets. Local archive presence
  is not proof of public publication or completed old-repository cleanup.
- Native per-project home routing is landed in
  [scripts/scodex.py](./scripts/scodex.py), preserving native schema and locks.
  Direct native/gateway startup still needs explicit home routing. There is no
  automatic one-file conversion or knowledge import.
- Hierarchy, context maps, blackboards, bounded root context, direct memory
  controls, eligible native reads and Ask/Work surfaces exist in source.
- Selected loopback gateway security and browser memory/draft controls landed
  on current APIs. The whole historical web branches were not merged.
- Canonical project-store/domain components and offline capsule experiments
  exist but remain inactive for native cutover/context activation.

See [architecture](./ARCHITECTURE.md) and
[configuration](./CONFIGURATION.md) for exact distinctions.

## Source, installed artifacts and verification are separate

| Layer | Evidence in this documentation pass |
| --- | --- |
| Source baseline | `997007527c5bb1d03c4d6b67cd5f8786b37b3e17`; package metadata `0.159.2` |
| Toolchain | Pinned Rust 1.95.0; observed Node 22.19.0, pnpm 10.34.5, Python 3.13.7, just 1.57.0 |
| Web suite | Windows **128/128 PASS**, including fixture-module discovery, no skipped tests |
| Launcher tests | Windows **10/10 PASS**, disposable/mocked launches, not model turns |
| Gateway walkthrough | Production HTTP handler with disposable synthetic bridge; not native storage qualification |
| Native CLI/companion | **UNVERIFIED**; no paired artifacts present, cold-build resource gate blocked |
| Native login/model/memory/cold restart | **UNVERIFIED**; not inferred from fixture results or a different installed binary |
| Rust scoped tests | Recipe/source checked, **execution UNVERIFIED** in this pass |
| WSL/Linux/macOS newcomer instructions | **UNVERIFIED** in this pass |
| Independent clean-clone follow | **Outstanding**; author checks are not fresh cross-engine review |

At build admission, free RAM was 20.11 GiB. The project requires more than
20 GiB free throughout builds; a 4 GiB cold-build reserve above that floor was
unavailable. No Rust process was launched or killed to force this gate.
See [development](./DEVELOPMENT.md) and [testing](./TESTING.md).

## Consolidation landings

| Commit | Scope |
| --- | --- |
| `197f59894e14be99284deb88081eac1386f9bb1c` | Documentation-only operation-specific compaction hold accounting |
| `0b6ab0cc57e9d5cc2fca314e273e33d8e14ee827` | Gateway admission/reply policy |
| `18a5205e064e3f2dd804a11126d6faae7a89185a` | Cookie/HTTP/RPC/reply integration |
| `c4055350b070fd6538bc9f202f58782ff13131a0` | Revision-keyed persistent drafts |
| `8e3747b1120b3c4a5a9ad644f670b0522b5a113b` | Direct memory controls/excerpt guard and default-port repair |
| `37ea0d07d9a26fbb11d34a30d1fd1559e86638b3` | Add actions, paging and refresh/save reconciliation |
| `997007527c5bb1d03c4d6b67cd5f8786b37b3e17` | Malformed restored-draft repair, HTTP memory contracts and client guidance |

The [consolidation ledger](./STATEFUL_CODEX_CONSOLIDATION.md) maps source OIDs
and archive tags to these selective landings. Item6 indexing/attribution remains
an unresolved integration-versus-archive decision pending its requested review.

## Holds and limitations retained

Compaction's archived stack is **not integrated or release-qualified**.
Recorded CLI, app-server, stateful-extension and core suites failed, alongside
an earlier broader-memory failure. The original controller result remains
**23 total / 8 PASS / 15 ERROR / exit 1**. Later source approval does not
override **RELEASE NO** or operation-specific holds.

Exact historical recall, packet restoration, exact-input correction and
post-Forget replay across all surfaces remain unqualified. Current correction
still trims input. Forget retires effective memory; it is not transcript erasure.
Foreign agents remain read-only; generic APIs are not a safe foreign-write
authorization contract.

B0 accounting/recovery and populated migration 59/60 compatibility remain
unqualified. No upstream batch promotion is claimed. Further installation,
ordinary-policy/Work-v3/PartB activation, matched-216 release panel, large-tree UI,
multi-user security, offline/HAL/customer acceptance remain held or unmeasured.
The store-root journey's original **overall exit 1** is retained despite its
successful scoped placement/read components; it is not regraded by these docs.

The [110-item roadmap](./STATEFUL_CODEX_ROADMAP.md) supplies the full inventory.
[Decisions](./STATEFUL_CODEX_DECISIONS.md) explains archive-versus-merge choices.

## History, not current instructions

The prior October 12 snapshot and October 16 inactive-kernel pointers described
earlier source/installation states. They are superseded by this page, not silently
reinterpreted as current claims. The pre-refresh page is recoverable from Git at
the runtime baseline above; it is not reproduced here because it mixed stale
operating instructions and machine-specific references.

Historical component witness files record narrow results only. In particular,
earlier canonical-domain passes do not prove current native routing/conversion,
and earlier HAL/Unicode failures remain failures. Historical measurements,
including negative and invalidated runs, remain in
[the evaluation ledger](./STATEFUL_CODEX_EVALUATION.md) and
[earlier evaluation history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md).
This refresh changes no evidence grade, release hold or product acceptance.
