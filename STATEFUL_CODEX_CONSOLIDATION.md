# Consolidation and historical recovery

Recorded **2026-10-20** at runtime baseline
`997007527c5bb1d03c4d6b67cd5f8786b37b3e17`.
One maintained branch, **`main`**. Archive tags are historical recovery refs,
not development branches, installed products or qualification claims.

The tables record **local preservation**, not public tag publication.
A public-history privacy audit has not been completed. A clean clone made with
`--no-tags` will not contain these refs. Obtain an owner-approved archive source
and the exact required ref before recovery; do not blanket-fetch/push all tags.
No private transcripts, blackboard contents, credentials or backup locations
are reproduced here.

## Final local counts for this phase

| Inventory | Count / disposition |
| --- | --- |
| Maintained local heads | **1**, `main` |
| Annotated archive tags | **89**: 75 local-head tips + 10 remote-only proposals + 2 stashes + 2 detached tips |
| Local-head tip categories | 21 formerly unmerged + baseline/53 other contained tips = 75 |
| Formerly unmerged dispositions | 3 selected subsets integrated, 17 archived, 1 (item6) pending review |
| Runtime integration commits after accounting | 6 selected web commits, not whole branch merges |
| Source worktree/remote-head/stash deletion | **None in this phase**; no completed global cleanup claimed |
| Archive publication / remote alignment | **UNVERIFIED/not performed** |

Tags preserve **committed graphs only**. Uncommitted compaction evaluation and
seven upstream/B0 files have separate private custody; no new WIP snapshot tags
were created by the imported archive phase. Do not claim those dirty bytes are
recoverable from a source-tip tag. Historical recovery drills are not repeated
or inflated into new full-recovery qualification here.

## 21 formerly unmerged tips

Every OID below is the **source commit**, not the annotated tag object's OID.
`Integrated` means only the named residual behavior. The rest stays archived.

| Source OID | Archive tag | Landing / insight |
| --- | --- | --- |
| `d10057bac0dbaea9d56bff623659fafa6da2e695` | `archive/20261020/heads/fix/continuity` | Archived: curated foundation already landed; capped/unreviewed remainder not imported. |
| `dfff18633c604a08e6a639d56f88a02a66eaec3c` | `archive/20261020/heads/fix/web` | Integrated security subset only, detailed below; unrelated historical UI preserved. |
| `9de1ef0052caad052f30fcef81d29844a511b5be` | `archive/20261020/heads/h1done/foundation` | Archived: superseded foundation and failing WIP. |
| `f7aebbc019a9d6f243a8e4b21a7cf75a52f65588` | `archive/20261020/heads/h1done/insession` | Archived: consumed/capped/cut mechanisms are not revived. |
| `7be002b288bc6ac19225650b76d8bec8422d0fbd` | `archive/20261020/heads/h1done/item4` | Archived: incomplete capture/identity/parser and migration work. |
| `0b458025a22cc8d12e0ec0a8e47347b05e14307f` | `archive/20261020/heads/h1done/item5` | Archived rejection: regex confirmation is not user authorization. |
| `4418ddc737308968557b25ffd14998981cd57066` | `archive/20261020/heads/h1done/item5file` | Archived: replacement freshness worker lacks completed adapters/review. |
| `d265021d3ef20e4f5df00fd63c3fc114d4aaebc1` | `archive/20261020/heads/h1done/item6` | Preserved, final integration choice pending requested review; indexing/attribution value is not a license for a broad stale port. |
| `d0c7ea9c8a6dfeafc09b41d4c52d3fdb6e87ed7d` | `archive/20261020/heads/h1done/item7` | Archived: rejected predecessors, unreviewed reduction and reconnect snapshot failure. |
| `067272525f702f86ff59592c6a245cd98400e4d9` | `archive/20261020/heads/h1done/web-items123` | Integrated direct-memory/draft/paging subset on current APIs only. |
| `019589bc3cb188e37efb44df36a50e585e0ccceb` | `archive/20261020/heads/rebuild/acceptance-start-revision-20261017` | Archived: unqualified command-start accounting draft. |
| `e1db7324a6864d099ceee8261d0276a09788ba8b` | `archive/20261020/heads/rebuild/active-compaction-20261018` | Archived full dependency graph; four failing suites/controller failure and release hold retained. Dirty append is separate. |
| `4a2c965c507e0c33e9b3575c62b500767a9b3cc9` | `archive/20261020/heads/rebuild/compaction-continuity-20261020` | Integrated → `197f59894e14be99284deb88081eac1386f9bb1c`, documentation only. |
| `4b3226b420a5761eb888ee15ba7d5522b5cb924b` | `archive/20261020/heads/rebuild/compaction-provenance-20261017` | Archived aggregate component; no duplicate replay. |
| `5b159609bec885da6c32bcc92e3b148633816c5b` | `archive/20261020/heads/rebuild/console-wire` | Archived codec draft overlaps landed successors. |
| `a71699214f5f52501144389b65f66d0aca5f4e32` | `archive/20261020/heads/rebuild/disposable-child-guard-20261017` | Archived aggregate component; startup/guard qualification held. |
| `7a27b80f0eb4019bdea051fadfa1cd33989f37c6` | `archive/20261020/heads/rebuild/exact-correction-20261017` | Archived useful repair; native qualification unresolved, not landed. |
| `d46d9fd5c62d865df6d708fb7cb26f3b20598670` | `archive/20261020/heads/rebuild/primary-integration-20261017` | Archived aggregate; no duplicate replay. |
| `c413cf43898ff4dc56b1d9e176a478f47d723da7` | `archive/20261020/heads/rebuild/test-client-path-20261017` | Archived unqualified platform/PATH draft. |
| `1b5142227a159fcb8ff7a93c8deea1421484c5ec` | `archive/20261020/heads/rebuild/tui-exact-correction-20261017` | Archived ingress/paste draft; actual refusal snapshot unqualified. |
| `04a51b086571b1a057b7c585843b14fa709671eb` | `archive/20261020/heads/sync/upstream-20261015` | Archived B0/populated-history/migration work; seven dirty files are separate, promotion held. |

## Selected source-to-landing map

The source commits below are ancestors of the named recovery tips, not additional
archive tags. Landings are adaptations, not wholesale cherry-picks.

| Source OID | Recovery tag suffix after `archive/20261020/heads/` | Landing commit(s) |
| --- | --- | --- |
| `0e3043b145f1e6a0b0d6e2c0bb79daff651184f0` | `fix/web` | `0b6ab0cc57e9d5cc2fca314e273e33d8e14ee827`, `18a5205e064e3f2dd804a11126d6faae7a89185a`; default-port repair in `8e3747b1120b3c4a5a9ad644f670b0522b5a113b`. |
| `ada6cf5ee3d023050a290ac36942dc2ce136f9e5` | `h1done/web-items123` | `8e3747b1120b3c4a5a9ad644f670b0522b5a113b`, browse/correct/Forget on current schemas. |
| `63d14f303fca11abed6c1bcd8e39b530cc77e377` | `h1done/web-items123` | `8e3747b1120b3c4a5a9ad644f670b0522b5a113b`, excerpt correction guard. |
| `1287b32203ba01300abec9921b63100cfb21070c` | `h1done/web-items123` | `c4055350b070fd6538bc9f202f58782ff13131a0`, `8e3747b1120b3c4a5a9ad644f670b0522b5a113b`, `37ea0d07d9a26fbb11d34a30d1fd1559e86638b3`, revision-keyed drafts/Add/paging. |
| `c056ab982a90c7a8d0502c7929423ad6222a2312` | `h1done/web-items123` | Same three landings, whole UTF-8 drafts, late-save retention and editor focus. |
| `f94499a3aaf73b2835cfab039a83fbf4be819819` | `h1done/web-items123` | `c4055350b070fd6538bc9f202f58782ff13131a0`, `37ea0d07d9a26fbb11d34a30d1fd1559e86638b3`, action identity changes only with changed words. |
| `067272525f702f86ff59592c6a245cd98400e4d9` | `h1done/web-items123` | Same two landings, settle only the completed addition. |

Supplemental current-source repair:
`997007527c5bb1d03c4d6b67cd5f8786b37b3e17` validates restored draft members
and adds HTTP memory-contract coverage. It imports no additional source family.
Qualification limits are in [current state](./STATEFUL_CODEX_CURRENT_STATE.md).

Review follow-up:
`b8d095182c703849c1371b35f9232d2cd4867d78` strengthens gateway and excerpt
regressions and aligns the disposable fixture with native refusal of Add
retries after Forget or correction. Windows web checks pass **132/132**.
Six isolated guard-removal mutations each fail their affected tests. This
changes tests/fixtures only, imports no additional archive source and does not
qualify native persistence or lift any release hold.

## Baseline and 53 other contained local tips

All 54 sources in this section are already ancestors of baseline
`0a5fbb5a30045eae47dbe809fc439bb730a2184b`.
Disposition is **already contained + archive preserved**, no new integration
commit or activation. Each full tag name identifies its original branch.

| Source OID | Archive tag |
| --- | --- |
| `0a5fbb5a30045eae47dbe809fc439bb730a2184b` | `archive/20261020/heads/main` |
| `4ea34e7d1058c2f64620596d91393b25a5137833` | `archive/20261020/heads/checkpoint/stateful-feature-20261015` |
| `00b62fe3208d529fa663b6673c5fe6b1fd12d7de` | `archive/20261020/heads/checkpoint/stateful-pre-upstream-20261015` |
| `83eee86b67189b6b9273fad07b525eb6757b4f0e` | `archive/20261020/heads/cleanup/entropy` |
| `0b000b29bc5aadea1b38a2c1337b7e8a134468bf` | `archive/20261020/heads/docs/oct-campaign` |
| `25f2a92259315a36f9d925a0aa36dbb6da7ac71b` | `archive/20261020/heads/rebuild/bounded-frame-io-20261018` |
| `0a5fbb5a30045eae47dbe809fc439bb730a2184b` | `archive/20261020/heads/rebuild/bounded-stderr-20261018` |
| `0a5fbb5a30045eae47dbe809fc439bb730a2184b` | `archive/20261020/heads/rebuild/bounded-transport-20261018` |
| `0a5fbb5a30045eae47dbe809fc439bb730a2184b` | `archive/20261020/heads/rebuild/journey-deadlines-20261018` |
| `0a5fbb5a30045eae47dbe809fc439bb730a2184b` | `archive/20261020/heads/rebuild/leased-writer-20261018` |
| `0a5fbb5a30045eae47dbe809fc439bb730a2184b` | `archive/20261020/heads/rebuild/owned-child-20261018` |
| `0a5fbb5a30045eae47dbe809fc439bb730a2184b` | `archive/20261020/heads/rebuild/owned-pumps-20261018` |
| `f583d4fd59d6f88e4ae96c2992d501ec460abce4` | `archive/20261020/heads/rebuild/shared-transport-20261018` |

The following **41 distinct tags** share source OID
**`6a1e962faab6f8beb13d4afbc874e7658f9486fe`** and the same
already-contained/archive disposition. Shared OIDs do not collapse tag counts.
The tags alone do not establish activation of their namesake components.

- `archive/20261020/heads/feature/stateful-codex`
- `archive/20261020/heads/rebuild/acceptance-snapshot`
- `archive/20261020/heads/rebuild/attributed-content`
- `archive/20261020/heads/rebuild/bootstrap-reader-final`
- `archive/20261020/heads/rebuild/bounded-request`
- `archive/20261020/heads/rebuild/canonical-steering-index`
- `archive/20261020/heads/rebuild/completion-learning-guard`
- `archive/20261020/heads/rebuild/console-codec`
- `archive/20261020/heads/rebuild/console-response`
- `archive/20261020/heads/rebuild/console-sequence`
- `archive/20261020/heads/rebuild/console-stream`
- `archive/20261020/heads/rebuild/domain-installation`
- `archive/20261020/heads/rebuild/domain-installer`
- `archive/20261020/heads/rebuild/domain-snapshot`
- `archive/20261020/heads/rebuild/domain-upgrade`
- `archive/20261020/heads/rebuild/effective-read-extraction`
- `archive/20261020/heads/rebuild/extension-gates`
- `archive/20261020/heads/rebuild/hal-utf8-defaults`
- `archive/20261020/heads/rebuild/knowledge-owner`
- `archive/20261020/heads/rebuild/knowledge-quiescence`
- `archive/20261020/heads/rebuild/knowledge-write-snapshot`
- `archive/20261020/heads/rebuild/native-attachment`
- `archive/20261020/heads/rebuild/native-cutover-admission`
- `archive/20261020/heads/rebuild/owner-manifests`
- `archive/20261020/heads/rebuild/per-project-store`
- `archive/20261020/heads/rebuild/platform-qualification`
- `archive/20261020/heads/rebuild/project-run-creation`
- `archive/20261020/heads/rebuild/project-runtime-transaction`
- `archive/20261020/heads/rebuild/project-store-bootstrap-2129`
- `archive/20261020/heads/rebuild/project-store-root`
- `archive/20261020/heads/rebuild/project-verification`
- `archive/20261020/heads/rebuild/project-writer`
- `archive/20261020/heads/rebuild/read-capabilities`
- `archive/20261020/heads/rebuild/registered-reader`
- `archive/20261020/heads/rebuild/request-parameters`
- `archive/20261020/heads/rebuild/response-frames`
- `archive/20261020/heads/rebuild/response-stream`
- `archive/20261020/heads/rebuild/run-start-binding`
- `archive/20261020/heads/rebuild/store-filesystem-admission`
- `archive/20261020/heads/rebuild/store-migration-registry`
- `archive/20261020/heads/rebuild/terminal-composition`

## Two stashes and two detached tips

| Source OID | Archive tag | Disposition / insight |
| --- | --- | --- |
| `568a7a186592040dfe43e16cf87e859bf7df1719` | `archive/20261020/stashes/knowledge-reader` | Full three-parent graph including index/untracked parent; successor equivalence unproved, not applied to main. |
| `6d3b5e949e166e227bcbfbd5278e0788fc68f918` | `archive/20261020/stashes/store-registry` | Full three-parent unfinished registry/dependency state; not applied to main. |
| `5eacf03387cd56d149b2775a21a59b0f04274907` | `archive/20261020/detached/chip_stateful_build` | Archive ref only; protected checkout/object-host dependency untouched. |
| `9fa6256bcd5a1001d39b59a4a3f05e28f59c6016` | `archive/20261020/detached/sc_wt_rebuild_knowledge_baseline` | Archive ref only; no source-worktree operation or new integration. |

## Ten dependency proposals, all archived

These are preserved proposals, **not reviewed upgrades**. No PR closure,
remote deletion or Dependabot/security-update setting change follows.

| Source OID | Archive tag |
| --- | --- |
| `48534ac53707efb1d29ed0eedb810c06fe5bf210` | `archive/20261020/remote/dependabot/cargo/codex-rs/flate2-1.1.10` |
| `cdd2450c8debf570d30a031752fb691ab0064934` | `archive/20261020/remote/dependabot/cargo/codex-rs/quick-xml-0.42.0` |
| `f67b39b2282916453ef9e1ab75d0bb6d5d98b460` | `archive/20261020/remote/dependabot/cargo/codex-rs/test-case-3.4.0` |
| `77909a5d158bc2e58ea621ce2ae59e9098c2d7df` | `archive/20261020/remote/dependabot/cargo/codex-rs/time-0.3.55` |
| `7c497f787238b39d715649008d2dc477c1f01db8` | `archive/20261020/remote/dependabot/cargo/codex-rs/unicode-segmentation-1.13.3` |
| `83417b573580cbd97592bfe691a018a90c3d86b3` | `archive/20261020/remote/dependabot/github_actions/actions/upload-artifact-7.0.2` |
| `dc9b908b530bbc81695cbdc5d9b27f7d05c297cd` | `archive/20261020/remote/dependabot/github_actions/astral-sh/setup-uv-10.2.0` |
| `6b2a0863ccd8971d32320cb9c30d2031ac687b22` | `archive/20261020/remote/dependabot/github_actions/codespell-project/codespell-problem-matcher-1.2.0` |
| `bbc923cb347fc03398bf3855fd4514b76f533b2e` | `archive/20261020/remote/dependabot/github_actions/openai/fence-0.10.5` |
| `f5845bf2ec3f721c4195fc2b5da8b3fffb44bb09` | `archive/20261020/remote/dependabot/github_actions/pnpm/action-setup-fc06bc1257f339d1d5d8b3a19a8cae5388b55320` |

## Recover selected source without disturbing main

Run from the canonical clone containing the approved local tag, in PowerShell.
This **selected-file recovery was executed** and compared byte-for-byte to its
Git blob in this documentation pass. It does not restore a whole private home,
stash index or uncommitted payload:

```powershell
$tag = "archive/20261020/heads/rebuild/exact-correction-20261017"
git rev-parse "${tag}^{commit}"
# Expected: 7a27b80f0eb4019bdea051fadfa1cd33989f37c6
$recovery = Join-Path ([IO.Path]::GetTempPath()) ("stateful-recovery-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $recovery | Out-Null
git -c core.autocrlf=false archive --format=tar --output="$recovery/correction.tar" $tag -- codex-rs/ext/stateful/src/memory_controls.rs
tar -xf "$recovery/correction.tar" -C $recovery
```

The command-scoped `core.autocrlf=false` preserves blob bytes on Windows without
changing repository settings. An initial archive using the local `autocrlf=true`
setting failed the byte comparison; the corrected command above is the verified
recovery recipe, not an unchanged-command pass.

Review the recovered source against current `main`, then propose a small port
with its own tests. Do not develop on archive tags or directly apply an entire
old tree to a dirty checkout. Exact tag fetch from an approved source and
whole-graph/stash restoration are **UNVERIFIED in this documentation pass**.
Archive publication/privacy and dirty-payload recovery require their separate
owner-controlled process; this page provides no authority to publish or delete.
