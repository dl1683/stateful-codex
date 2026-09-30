# Day-one acceptance invalidated

The senior review of the first harness run invalidated its reported `5/6` result.
The evidence supports `0/6` complete scripted conversations: prompts were merged or
never delivered, fixtures inherited the harness repository, and missing artifacts
were manufactured as empty files. This note is the baseline for the rebuilt gate;
the old DAY_ONE scorecard must not be used as product evidence.

The rebuilt acceptance must report base and stateful arms separately and count a
conversation only when its infrastructure, message-boundary, rollout, state, and
workspace evidence gates all pass.
