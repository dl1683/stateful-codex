# Stateful Codex longitudinal evaluation protocol

This protocol measures whether durable project intelligence improves work over
time. It is not a search for five favorable demos. The product-intent document
defines the desired outcome; this file freezes how the next evidence is
collected.

## Experimental unit and selection

- The project trajectory is the clustered experimental unit. Twenty questions
  from one project are correlated observations, not twenty independent samples.
- Six projects are selected for domain and workload coverage before comparative
  outcomes are inspected. Five run the complete longitudinal sequence. The
  sixth is a reserved replication project.
- Every selected project, exclusion, interruption, and failed run remains in the
  report. A failed project is not silently replaced by a more favorable one.
- The repeated procurement continuity case is a frozen development case and is
  excluded from selection and aggregate claims.

## Matched arms

Each project has two arms that begin from empty project state:

1. ordinary Codex in one continuous thread; and
2. Stateful Codex in one continuous thread.

The arms use isolated copies of the same corpus, the same ordered questions and
source revisions, model, reasoning effort, harness build, permissions,
authentication mode, retry policy, and execution budget. Per-turn corpus hashes
prove source equivalence; filesystem paths may differ solely because the copies
are isolated. The user still selects the project, thread, and mode. The
evaluator does not infer them.

Question order is frozen before either arm is run. The sequence includes:

- early corpus-learning questions;
- later questions that should benefit from accumulated understanding;
- non-obvious cross-source deductions and contradictions;
- at least one source revision that invalidates an earlier conclusion;
- questions whose correct supported answer is unknown or indeterminate;
- a natural compaction boundary when the workload reaches one; and
- a separately labelled restart/resume checkpoint.

Fresh-thread transfer is a separate experiment. It must not replace the
continuous-thread primary arms because that would avoid the compaction behavior
the longitudinal test is intended to measure.

## Rollout-derived measurements

`compare-longitudinal-rollouts.mjs` reads one append-only rollout per arm and
attributes records by turn ID. It reports:

- response-level usage summed within each turn, plus reported turn and thread
  totals for reconciliation;
- full, cached, uncached, output, reasoning-output, and uncached-plus-output
  tokens per turn and cumulatively;
- model responses, outer tool calls, read operations, evidence routes, repeated
  evidence reads, rejected tool results, and completion attempts;
- canonical `compacted` records and legacy compaction signals without treating
  the legacy duplicate as another compaction;
- duration and time to first token;
- the actual model-visible `<stateful_project>` fragment at the first and last
  response, including byte size, root byte size, revision, included and omitted
  root entries, evidence routes, and rendered freshness; and
- durable Stateful completion observed in the same turn.

The evaluator rejects missing or incomplete turns, prompt/configuration drift,
unattributed compactions, unreconciled usage, a missing Stateful project
fragment, and missing durable completion. Cumulative session totals are never
summed once per question.

## Measurements that require an external observation

Rollouts do not prove full deep-state size, every raw-source region read, or
semantic quality. A case-keyed observation JSON file must therefore accompany
each arm. Required top-level fields are configured in the frozen manifest.

```json
{
  "cases": {
    "q01": {
      "quality": {
        "blinded": true,
        "rubricVersion": "stateful-longitudinal-v1",
        "scores": {
          "visibleAnswer": {
            "correctness": 4,
            "evidenceTraceability": 4,
            "decisiveDetail": 3,
            "uncertaintyCalibration": 4,
            "contradictionAndFreshness": 4,
            "usefulness": 4
          },
          "durableState": null
        }
      },
      "sourceAudit": {
        "uniqueFilesRead": 4,
        "exactRegionsRead": 5,
        "repeatedRegions": 1,
        "broadReads": 0,
        "corpusRevision": "sha256:..."
      },
      "state": null
    }
  }
}
```

For the Stateful arm, `quality.scores.durableState` is scored separately and
`state` records the post-turn project revision and counts for hierarchy nodes,
blackboard entries, context-map entries, relationships, root entries, stale
records, and maintenance work. Ordinary Codex uses `null` for durable state and
does not require the `state` field.

Missing observations remain missing and make the comparison invalid. They are
never reconstructed from answer prose or replaced with a literal term score.

## Frozen semantic rubric

Each dimension is scored from 0 to 4 against the exact source corpus:

- `correctness`: material conclusions are correct;
- `evidenceTraceability`: consequential claims route to sufficient exact
  evidence;
- `decisiveDetail`: the response finds and uses the facts or cross-source
  connections that change the outcome;
- `uncertaintyCalibration`: unknowns, assumptions, and confidence are stated at
  the right strength;
- `contradictionAndFreshness`: conflicts and changed sources are recognized and
  stale conclusions are repaired; and
- `usefulness`: the result directly resolves the user's requested outcome and
  preserves the relevant boundary.

Graders receive randomized arm labels, the prompt, source revision, and artifact
being scored. They do not receive token counts or product-arm identity. Visible
answers and Stateful durable records are graded as separate artifacts. Literal
expected-concept and forbidden-phrase checks remain diagnostic only.

## Reporting

Report every turn and every project. Show marginal and cumulative cost,
quality, reads, state growth, freshness, compactions, and retry behavior by
question number. Aggregate token totals are descriptive; project win counts are
reported without treating the 100 turns as independent samples. Provider cache
behavior, maturation/maintenance cost, interruptions, and source changes remain
visible.

Published Codex or Luna benchmark scores are contextual unless benchmark
version, model, harness, prompt budget, retry policy, tools, and scorer are
demonstrably comparable. No causal advantage is claimed from unlike public
scores.

## Command

```text
npm run eval:longitudinal -- \
  --manifest eval/manifests/longitudinal-suite.json \
  --pair PROJECT=ordinary-rollout.jsonl,stateful-rollout.jsonl \
  --observations PROJECT=ordinary-observations.json,stateful-observations.json
```

Repeat `--pair` and `--observations` for each project in the frozen manifest.
The process exits nonzero when the comparison contract or required measurements
are incomplete; performance losses do not make a valid experiment invalid.
