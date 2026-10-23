# Stateful Codex decisions and failure lessons

Single decision/failure record. Evidence, counts and their qualifications live
in [RESULTS](RESULTS.md); current contracts/scope in
[ARCHITECTURE](ARCHITECTURE.md); operating commands in
[DEVELOPMENT](DEVELOPMENT.md). Consolidated 2026-10-22 without lifting holds.

## Do not repeat

| Pattern | Durable lesson / decision |
| --- | --- |
| Source exists, therefore delivered | Distinguish landed, inactive, held, rejected and archived. Component tests do not qualify native routes, migrations, offline or customers. |
| Correct answer, therefore valid journey | Check exact tool/raw/final outputs, durable result and exit independently. LF drift, nested double escaping, wrong envelopes and encoding failures remain failures. |
| Exit zero, therefore accepted | Answered is unverified. Completed needs preserved durable acceptance; synthetic receipts cannot certify a real checker. |
| Source inspection, therefore runtime acceptance | Preserve failed normal suites and native execution measurements. Release remains unqualified; disabling failing compaction does not establish acceptance. |
| Saved, therefore applied | Separate capture, eligibility, model exposure, authority, scope and obedience. Receipts must identify omission/proposal versus saved words. |
| Forgotten, therefore erased | Retire active copies and prevent resurrection across reads, summaries, outcomes and background extraction; original history and already-sent context remain unless an actual erasure contract runs. |
| Fresh bytes, therefore current conclusion | Check governing-source coverage and source order, not only fingerprint. HEAD/size/mtime miss same-size edits, new governing files and authority changes. |
| A prompt quote or regex confirms authority | Reject automatic human-rule promotion from model/source/quotation/confidence/regex cues. Deliberate Apply/direct control and provenance are required. |
| Agent wrote it, then source verified it | Preserve causal lineage; a self-authored source is not independent corroboration. Origin and freshness are different. |
| Generic SQL/RPC is a foreign write contract | Foreign agents remain read-only. Human direct add/correct authority cannot be borrowed by a foreign caller. |
| Independent connections prove process/crash safety | Force actual contention/cancellation windows and observe close/reap/durable recovery separately. Never infer power-loss safety. |
| Matching table counts prove migration | Preserve revisions, triggers, FTS, journals, semantic edges, identities and cross-domain closure. Incomplete copies passed counts/integrity while losing material rows. |
| A clock guard blocks all old writers | Clock-only guards permitted timestamp mutation. Runtime row guards do not cover PI/metadata, DDL, execution stopping or coherent multi-DB import. |
| Root-only filesystem checks suffice | Qualify actual directory, DB and sidecars before opening; stable descendant submounts defeated the first opener. Same-user races/sync-agent absence remain limits. |
| Async Drop proves cleanup | It may request termination/abort, not await reap/join. Record only observed completion and preserve cancellation/uncertain-ACK state. |
| Wrapper Completed proves transport success | Preserve partial EOF/I/O failures independently from wrapper/task lifetime status. |
| Re-run until it passes | Retain original failures, separate prospective repaired attempts and disclose rubric changes. |
| Cheap marginal turn proves savings | Include maturation, capture, retries, compaction, maintenance and user work. Full/cached/uncached tokens, synthetic units, list-price estimates and bills are distinct. |
| Tuned recall proves generalization | Freeze unseen probes; once inspected/tuned, a set becomes regression evidence. Capsule gains failed on held-out data. |
| Lower benchmark gap proves parity | Unmatched public models/harnesses, proxy judges and changed denominators cannot prove equivalence or Stateful advantage. |
| Unicode codec fixes native delivery | ASCII-safe JSON, file UTF-8 defaults, console inheritance and exact final model rendering are separate gates; the native 405→329-byte failure remains. |
| Successful full-access control fixes restricted path | Preserve restricted failure and sandbox/authorization. No policy/backend/identity workaround is acceptance evidence. |
| Old plan says “next” | Use README status, not dated orders or handoffs. Archived work requires a bounded port against current APIs. |

## Chosen boundaries

Only selected small historical ports entered canonical main: operation-specific compaction
hold accounting, loopback gateway security, revision-keyed browser memory controls,
drafts, Add/paging and regression repairs. Whole historical branches contain stale
APIs, rejected mechanisms and unqualified startup/migration behavior.
Archive them without delivering them; do not import old knowledge or restart the
retired one-file rebuild. Native project-local placement preserves native schema
and locks, not canonical conversion.

One maintained branch, **main**. Upstream changes arrive in pinned reviewed/tested
batches; checked/fetched/merged/validated/promoted remain separate.
Accounting/recovery checks and populated migration fixtures do not authorize promotion.
Do not rewrite applied migration identity/checksums or overwrite main with upstream.
The upstream integration does not activate archived candidate features.

Public archive publication, remote deletion/alignment, repository settings,
security-update behavior and independent clean-clone review require their own
evidence/authority. This cleanup publishes none of them. Foreign and sealed custody
remain untouched; only verified, archived duplicate checkouts are removed by the
authorized consolidation. Dirty payload custody is not commit-tag recovery.
No blanket-fetch/push of historical tags.

## Held acceptance

Exact recall, correction/Forget replay, populated migrations, foreign writes,
reliable Work completion and full native/platform acceptance remain unqualified.
The [results ledger](RESULTS.md) retains their measurements and failed journeys.
Archived designs are not current implementation or permission to activate them.

The October 23 pre-merge comparison reproduced both predecessor fan-in
60-second timeouts at `43a2c15951`; these are pre-existing Windows performance
limits, not qualified passes. It also reproduced nine rollout/indexing fixture
failures, one guardian history fixture failure and two answered-turn fixture
failures. The merged fixture rechecks passed, but do not erase those baseline
failures or establish populated-store or native acceptance. Counts and the
separate merge-induced compaction findings are retained in [RESULTS](RESULTS.md).

The final matched checks leave app-server fan-in timing out on both revisions.
Extension fan-in and exec-image passed focused with deadline sensitivity;
PI fan-in passed focused on both revisions but its broad-only timeout attribution
remains uncertain. The latest per-case union is 2,059 passes and one timeout,
assembled across runs, with original leak/retry warnings retained. See the
[final classification](RESULTS.md#final-integration-regression-classification-october-23).

## Recovery without reviving stale work

The October 20 preservation ledger recorded **89 annotated local tags**:
75 local-head tips (21 formerly unmerged plus baseline/53 contained), ten
remote-only dependency proposals, two stashes and two detached tips.
This is a historical custody count, not proof that every clone has them or that
they were published. A `--no-tags` clone lacks them. Obtain only the exact
owner-approved archive ref from an approved source.

Useful non-contained source tips:

| Tag suffix after `archive/20261020/heads/` | Source OID | Disposition |
| --- | --- | --- |
| `fix/continuity` | `d10057bac0dbaea9d56bff623659fafa6da2e695` | Curated foundation already landed; remainder archived |
| `fix/web` | `dfff18633c604a08e6a639d56f88a02a66eaec3c` | Security subset ported, not whole UI |
| `h1done/foundation` | `9de1ef0052caad052f30fcef81d29844a511b5be` | Superseded/failing WIP archived |
| `h1done/insession` | `f7aebbc019a9d6f243a8e4b21a7cf75a52f65588` | Consumed/capped/cut work archived |
| `h1done/item4` | `7be002b288bc6ac19225650b76d8bec8422d0fbd` | Incomplete capture/identity/parser/migration |
| `h1done/item5` | `0b458025a22cc8d12e0ec0a8e47347b05e14307f` | Rejected regex authorization |
| `h1done/item5file` | `4418ddc737308968557b25ffd14998981cd57066` | Freshness-worker adapters/review incomplete |
| `h1done/item6` | `d265021d3ef20e4f5df00fd63c3fc114d4aaebc1` | Review pending |
| `h1done/item7` | `d0c7ea9c8a6dfeafc09b41d4c52d3fdb6e87ed7d` | Rejected predecessors/reconnect snapshot failure |
| `h1done/web-items123` | `067272525f702f86ff59592c6a245cd98400e4d9` | Memory/draft/paging subset ported |
| `rebuild/acceptance-start-revision-20261017` | `019589bc3cb188e37efb44df36a50e585e0ccceb` | Unqualified draft |
| `rebuild/active-compaction-20261018` | `e1db7324a6864d099ceee8261d0276a09788ba8b` | Full failing dependency graph archived; release unqualified |
| `rebuild/compaction-continuity-20261020` | `4a2c965c507e0c33e9b3575c62b500767a9b3cc9` | Accounting only → `197f59894e14be99284deb88081eac1386f9bb1c` |
| `rebuild/compaction-provenance-20261017` | `4b3226b420a5761eb888ee15ba7d5522b5cb924b` | Aggregate component archived |
| `rebuild/console-wire` | `5b159609bec885da6c32bcc92e3b148633816c5b` | Codec predecessor archived |
| `rebuild/disposable-child-guard-20261017` | `a71699214f5f52501144389b65f66d0aca5f4e32` | Startup/guard unqualified |
| `rebuild/exact-correction-20261017` | `7a27b80f0eb4019bdea051fadfa1cd33989f37c6` | Useful exact-input repair, not landed |
| `rebuild/primary-integration-20261017` | `d46d9fd5c62d865df6d708fb7cb26f3b20598670` | Aggregate archived |
| `rebuild/test-client-path-20261017` | `c413cf43898ff4dc56b1d9e176a478f47d723da7` | Platform/PATH draft |
| `rebuild/tui-exact-correction-20261017` | `1b5142227a159fcb8ff7a93c8deea1421484c5ec` | Ingress/paste/refusal snapshot unqualified |
| `sync/upstream-20261015` | `04a51b086571b1a057b7c585843b14fa709671eb` | B0/migration held; seven dirty files separate |

Selected adaptations, not wholesale cherry-picks: security source `0e3043b145f`
became `0b6ab0cc57e9`/`18a5205e064e`; direct controls `ada6cf5ee3d` and excerpt
guard `63d14f303fc` became `8e3747b1120`; draft/Add/paging sources `1287b32203b`,
`c056ab982a9`, `f94499a3aaf`, `067272525f7` became `c4055350b070`,
`8e3747b1120`, `37ea0d07d9a`. Current-source draft/HTTP repair is `997007527c5`;
test/fixture regression follow-up `b8d095182c7` adds no archived runtime activation.

The other 54 local tips were already contained by baseline
`0a5fbb5a30045eae47dbe809fc439bb730a2184b`; 41 distinct tags shared
`6a1e962faab6f8beb13d4afbc874e7658f9486fe`. Shared OIDs do not collapse tag
counts or qualify the named components. The complete 89-ref/OID table remains
in the pre-cleanup consolidation record, not a second current document.

Stash graphs `568a7a186592040dfe43e16cf87e859bf7df1719`
(`archive/20261020/stashes/knowledge-reader`) and
`6d3b5e949e166e227bcbfbd5278e0788fc68f918` (`stashes/store-registry`)
retain three parents including index/untracked state, but not arbitrary later
dirty payloads. Detached tags preserve `5eacf03387cd56d149b2775a21a59b0f04274907`
(`detached/chip_stateful_build`) and `9fa6256bcd5a1001d39b59a4a3f05e28f59c6016`
(`detached/sc_wt_rebuild_knowledge_baseline`), not permission to delete protected
worktrees/object hosts. Ten archived dependency proposals are not approved upgrades,
PR closures or permission to disable security updates.

Selected-file recovery, historically executed with byte equality:

```powershell
$tag = "archive/20261020/heads/rebuild/exact-correction-20261017"
git rev-parse "${tag}^{commit}"
# Expected: 7a27b80f0eb4019bdea051fadfa1cd33989f37c6
$recovery = Join-Path ([IO.Path]::GetTempPath()) ("stateful-recovery-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $recovery | Out-Null
git -c core.autocrlf=false archive --format=tar --output="$recovery/correction.tar" $tag -- codex-rs/ext/stateful/src/memory_controls.rs
tar -xf "$recovery/correction.tar" -C $recovery
```

An earlier archive with local autocrlf enabled failed byte comparison; the
command-scoped override preserves blobs without changing repository settings.
Review recovered code against current main and propose a bounded port with tests.
Whole-graph/stash/dirty-payload restoration and remote archive retrieval remain
separate, unqualified operations. To inspect a removed document itself, use
`git show pre-doclayout-20261022:<old-path>` or the private mirrored archive;
do not restore a whole old tree over live work.
