# Stateful Codex product-testing handoff

Updated: 2026-09-27

This covers the **product-testing and feedback workstream**. It complements the development handoff in the repo, `STATEFUL_CODEX_HANDOFF.md`, and does not replace it.

## Rules from Devansh

- **Where to file issues:** always on `dl1683/stateful-codex`. **Never** on `openai/codex`.
- **Models for A/B tests:** both arms (Stateful and base Codex) run `-m gpt-5.6-luna -c model_reasoning_effort="high"`. Real work uses the default model.
- **Auth:** Codex runs only via the ChatGPT login. Prefix commands with `env -u OPENAI_API_KEY -u CODEX_API_KEY`, and never use API keys.
- **What a verdict must answer:** is the answer better or comparable in quality? Is it cheaper? Does it operate better (rules kept, survives long runs and compaction, speed)? The model is identical, so quality should be comparable; don't over-invest in arm-fairness confounds.
- **Stateful test priorities:**
  - cross-surface memory (CLI ↔ headless ↔ web UI);
  - live, followable understanding changes;
  - steering;
  - transparent traces and cost (including headless);
  - use it as a user on complex work.
- **Test design:** agree what a test isolates **before** launching it. Devansh's core question is whether nuanced facts and instructions given mid-session ("remember this", "consider that") survive and get used many compactions later. Tests built from independent units (a backlog of self-contained items) can't answer that; they only show rule persistence.
- **Obligation behaviour:** the rolling obligation should be reworked *continuously* as the agent learns, like planning a legal defence from a case dump, not refreshed only at compaction.

## Where things are (all outside the repo)

| Path | What it is |
|---|---|
| `C:/Users/devan/sc_sqlite` | **Isolated Stateful store** (backfill complete). Always pass `-c sqlite_home=C:/Users/devan/sc_sqlite` so tests never touch the user's real `~/.codex` state (see #26, #29, #30). |
| `C:/Users/devan/sc_build` | Git worktree of the fork for test builds. |
| `C:/Users/devan/sc_build_target/debug/codex-exec.exe` | Headless build at `19222f0`. |
| `C:/Users/devan/sc_build_target2/debug/codex.exe` | CLI/TUI plus `codex-code-mode-host.exe` at `926f2ac`, the last commit whose TUI compiles (#38). |
| `C:/Users/devan/sc_ui_client/` | Scratch copy of `clients/stateful-codex` whose `server.mjs` adds `-c sqlite_home=$SC_TEST_SQLITE_HOME`. Run: `CODEX_BIN=C:/Users/devan/sc_build_target2/debug/codex.exe SC_TEST_SQLITE_HOME=C:/Users/devan/sc_sqlite node server.mjs` → http://127.0.0.1:4173 |
| `C:/Users/devan/sc_tui/drive.py` | PTY TUI driver (pywinpty + pyte). Usage: `python drive.py <exe> <workdir> <outprefix> <script.json>`; the notes are in `NOTES.md`. |
| `C:/Users/devan/sc_v2/` | Campaign scripts (`campaign*.sh`, `long*_run.sh`), timelines, answers and logs, analysis scripts (`cite_check.py`, `template_check.py`, `we_len.py`, `topic_tokens.py`, `build_packet.py`, `judge_pairs.sh`), and review notes in `review/` (v3 base versus Stateful). |

**Build recipe for test builds** (run at BelowNormal priority, and never while A/B campaigns are running):
- set `RUSTY_V8_ARCHIVE=<target>/debug/gn_out/obj/rusty_v8.lib`;
- set `RUSTY_V8_SRC_BINDING_PATH=<target>/stateful-v8-artifacts/src_binding_ptrcomp_sandbox_release_x86_64-pc-windows-msvc.rs`;
- set `AWS_LC_SYS_PREBUILT_NASM=1`;
- after changing `.sql` migrations, run `cargo clean -p codex-state -p codex-project-intelligence -p codex-stateful-runtime`.

Migration checksum line endings (#29) should be fixed by the uncommitted working tree; until that lands, the checkout's line endings must match the store (`state/migrations` CRLF, the others LF).

## Harness gotchas (each cost real time)

1. **Background `codex exec` must close stdin (`< /dev/null`),** or it hangs silently before creating a session (fork issue #1). Confirm a worker is live by its `~/.codex/sessions/<date>/rollout-*` file, not just its PID.
2. **To stop a campaign, kill the driving `bash campaign*.sh` loop too,** not only the codex processes. Otherwise a relaunch runs a duplicate series writing to the same files.
3. **`codex exec` "tokens used" is uncached input plus output.** Cumulative input and cached tokens appear in `--json` `turn.completed` or `turn.progress` usage.
4. **Compaction events do not appear in the `--json` stream.** Count `"type":"compacted"` records in the session rollout file. Each record's `replacement_history` shows exactly what survived.
5. **Stateful keys projects by content,** apparently. A re-copied fixture with an identical tree joined the earlier project's session thread. Use distinct content or directories per arm and per run.
6. **Windows file locks:** `git add` in fresh copies can hit transient "Permission denied" on `.git/objects`. Retry, and verify the baseline commit exists before launching.
7. **Don't time post-run checks** (tests, audits) as part of the agent time.
8. **Web UI:** every workspace page ends in a busy render loop (about 1.15 cores and 900 MB per tab) even on a 220-file project (#39). DevTools evaluation over 45 s fails. Read run results from the session rollout instead.

## Issues filed from testing (dl1683/stateful-codex)

- **#24:** headless opacity (progress, cost, contribution record).
- **#25:** evolve beyond Codex limits (logins, providers, computer use, transparency).
- **#26:** version skew; an older binary silently loses memory.
- **#27:** thread crossing surfaces. Partly resolved: the Continue picker lists TUI threads.
- **#28:** start-screen clutter and unstable re-rendering.
- **#29–#31:** migration line-endings, stock-Codex migration skew, and slow first-run backfill.
- **#32:** first A/B across 4 projects (+5% tokens, thin capture, a cited-but-wrong number).
- **#33, #36:** headless transparency and UX (tool actions not streamed; counters read 0; PDFs opaque to evidence).
- **#34:** headless resume unsupported. **#37:** concurrent sessions starve on the SQLite pool.
- **#35 (evidence):** positive results across four codebases. Stateful uses 20–54% fewer tokens after first contact on analyst, coding, JavaScript and research series, with comparable answers. First contact costs more. Stateful answers are sometimes shorter.
- **#38:** the interactive CLI does not compile since `1f55cc0`. **Still true in the uncommitted working tree,** which adds a second unhandled variant, `TurnTrajectoryUpdated`, alongside `StatefulAttributionCompleted`.
- **#39:** TUI and web UI report (prompt required at launch, placeholder branding, hierarchy freeze, excellent mid-run steering and obligation panel), with two follow-up comments.
- **#40:** staleness detection works, but **line citations are off by 2–25 lines** (4 of 4 checked cases); base Codex's were exact.
- **#41:** Socratic mode's first reply is a refusal; "resume implementation" is impossible headless.
- **#42:** a long single run took a scripted, templated shortcut (blinded judge preferred base 19/20).

## Latest long-run evidence (v3, 150 topics, not yet filed)

**Summary:** Stateful's prose was comparable to base, but its verification was weaker. It cost +25% uncached tokens and took +15% longer.

**Verification:**
- It fixed 1 of 5 real animation bugs that base fixed. It propagated one (a GP table contradicting its plot) into new text, and introduced one factual error.
- It kept the canonical section order across 5 compactions; base's order flipped after compaction 3.

**State tools barely used:**
- The obligation was **updated once at the start, then not again for 31 minutes and 34 topics, until compaction 1.** After that it was updated every 4–7 minutes.
- The blackboard stayed empty until the final two records.

**The re-anchoring effect:**
- The agent's own starting strategy ("ground worked examples in animation values") was ignored during topics 21–39, where unverified and wrong examples were kept.
- Right after compaction 1 re-injected it, behaviour switched to match.
- Also, the obligation is a rolling window, so early learnings roll off.
- **Worth filing as the product issue:** maintain the obligation continuously; promote durable learnings.

**Details:** `sc_v2/review/v3_sc_notes.md` and `v3_base_notes.md`.

## Suggested next test (not run; needs Devansh's agreement on the design)

Run one long session on genuinely cumulative work, such as a large case or document dump followed by a strategy draft:
- inject nuanced facts and instructions mid-session at known times;
- force several compactions;
- check whether later decisions honour them, and whether they were written to the obligation, the blackboard or unresolved steering.

Measure the obligation's update cadence throughout, not only at compaction.
