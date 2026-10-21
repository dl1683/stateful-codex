# Contributing to Stateful Codex

Use the [reading map](./README.md#reading-map), then read [AGENTS.md](./AGENTS.md).
This fork accepts Stateful changes in the
[canonical Stateful repository](https://github.com/dl1683/stateful-codex),
targeting **`main`**, not OpenAI's repository.

## First change

1. Clone canonical `main` as shown in the README. Read the current-state and
   decision pages before relying on an old plan or archive tag.
2. Pick one bounded behavior or documentation repair. State what will change,
   what must remain unchanged and which tests will show it. Check existing work
   before starting; do not duplicate an executor's in-flight files.
3. External contributors may create a short-lived topic branch in their fork.
   There is only **one maintained product branch, `main`**. Do not create a
   long-lived feature/rebuild/upstream integration line.
4. Follow nearby conventions and any more-specific `AGENTS.md`. Prefer existing
   crates rather than adding to `codex-core`. Keep complex patches under 500
   changed lines where practical; split mechanical/generated changes clearly.
5. Run the relevant checks from [TESTING.md](./TESTING.md). Record exact source
   and tool versions, command, exit, failed/skipped checks, and fixture versus
   native scope. Do not call unrun tests passed.
6. Inspect the diff and all staged paths. Ensure no private homes, credentials,
   account identifiers, evaluation transcripts or personal paths enter the PR.
7. Commit a small coherent change and prepare a PR targeting Stateful `main`.
   Include intent, behavior, test results, unresolved holds and any compatibility
   implications. A first documentation PR can be reviewed without native builds,
   but must not claim an unverified native walkthrough passed.

Example Git workflow, **UNVERIFIED against a contributor fork/remote here**:

```text
git switch -c docs/first-change
git diff --check
git add README.md
git diff --cached
git status --short
git commit
```

Run these from your own clone; substitute the exact edited file for `README.md`.
Do not switch over dirty work or stage unrelated changes. Submit through the
canonical repository's PR UI after publishing your fork branch yourself.
This documentation task does not authorize any push, PR creation or posting.

## Commit messages

Use one generic subject line, a blank line, then:

```text
Improve contributor guidance

Committed by <name>
```

`Committed by <name>` is optional for external contributors. Maintainer
consolidation commits use the owner's requested attribution. Preserve configured Git
author/committer identity; never override it to imitate another contributor.
Factory's harness additionally requires its bot `Co-authored-by` trailer;
executor reports must disclose that addition. Do not place personal email
addresses in documentation examples.

## Ownership and review

One executor owns each coherent implementation scope. Coordinate file custody
when others are working in the same checkout. On your own machine, check for
existing Cargo/rustc builds and wait for them to finish; only on a shared
maintainer machine must you coordinate builds with its other users. Follow
[development's process checks](./DEVELOPMENT.md#before-any-rust-build-or-rust-test).
Existing dirty/untracked work belongs to its author, even if it looks temporary.

The project practice is Droid design/implementation/build, followed by a
**different fresh real Codex CLI session** for independent tests and hands-on
review. A quota fallback must be explicitly authorized and recorded; do not
claim a same-engine subagent is that independent review. Subjective Luna scoring
is separate from correctness tests. One review per coherent feature is the
default, not a new approval bureaucracy.

For documentation, a fresh cross-engine reviewer receives only a clean clone
and README, follows the build/CLI/web/memory/Forget/test instructions, and
prepares an unsubmitted first patch. Record platform, artifacts, failures and
repairs. **That clean-clone independent review is outstanding**, not supplied
by this documentation author's local checks.

## Upstream changes enter main deliberately

Keep upstream OpenAI Codex as a source of pinned changes, not as the destination
for Stateful PRs. A maintainer records the upstream source OIDs and preservation
requirements, separates checked/fetched/merged/validated/promoted status, and
lands reviewed, tested batches into `main`. Preserve Stateful context ordering,
retry/delivery semantics, native history compatibility and exact API contracts.
Never overwrite `main` with upstream or assume a cached sync ref is current.

The archived October upstream work is **not approved for promotion**.
B0 accounting/recovery, populated migration 59/60 compatibility and later batch
validation remain held. Existing applied migrations must not be deleted or
have checksums rewritten. Dependency changes require Bazel lock regeneration;
API/config changes require their generators. See [testing](./TESTING.md).

Archive tags preserve unfinished work without delivering it. Reopening one
requires a new small proposal on current `main`, not development on the tag.
Publication, remote branch retirement, Dependabot settings and destructive
cleanup need explicit owner authority; this guide grants none.
