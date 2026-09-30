# Day-one acceptance invalidated

The senior review of the first harness run invalidated its reported `5/6` result.
The evidence supports `0/6` complete scripted conversations: prompts were merged or
never delivered, fixtures inherited the harness repository, and missing artifacts
were manufactured as empty files. This note is the baseline for the rebuilt gate;
the old DAY_ONE scorecard must not be used as product evidence.

The rebuilt acceptance must report base and stateful arms separately and count a
conversation only when its infrastructure, message-boundary, rollout, state, and
workspace evidence gates all pass.

## Rebuilt TUI acceptance: 2026-09-30

Run root: `eval/runs/REWORK_DAY_ONE_R5`.

The paired acceptance planned 12 attempts: six catalogue scenarios, one base arm
and one stateful arm per scenario, with at most three concurrent conversations.
The result was **0/12 valid conversations**. Every attempt reached the real TUI's
sandbox setup menu before a valid scripted turn. The driver recorded
`sandboxSetupRequired`, retained the final screen, and sealed the attempt as
infrastructure-invalid.

Manual inspection found no valid user/assistant rollout boundary in any attempt,
no readable Stateful project state for the stateful arms, and zero workspace
changes against the independent external Git baselines. SQLite snapshots and
preflight canaries were retained as capture evidence; they do not make a
conversation valid. The acceptance must be rerun after the sandbox capability is
made available to the executable.

## Rebuilt TUI acceptance: 2026-09-30 R7

Run root: `eval/runs/REWORK_DAY_ONE_R7`.

The paired acceptance completed **12/12 valid conversations**: six catalogue
scenarios, each run once by the base arm and once by the Stateful arm, with at
most three concurrent conversations. All attempts passed the infrastructure,
message-boundary, rollout, requested-state, evidence, workspace-isolation, and
sealed-store gates.

The first in-run scorecard showed 6/12 because its already-loaded grader
required an `available: true` wrapper that the native Stateful export does not
emit. Post-run regrading with the corrected `stateful-project-state-v1`
check produced the final 12/12 result. The corrected scorecard and each sealed
attempt's evidence directory are retained under the R7 run root.
