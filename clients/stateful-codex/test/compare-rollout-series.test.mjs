import assert from "node:assert/strict";
import test from "node:test";

import { compareSeries } from "../eval/compare-rollout-series.mjs";

const usage = (totalTokens, uncachedTotalTokens) => ({
  totalTokens,
  uncachedTotalTokens,
});

const summary = (id, tokens) => ({
  sessionId: id,
  originator: "codex-tui",
  source: "cli",
  model: "test-model",
  reasoningEffort: "high",
  approvalPolicy: "never",
  sandboxPolicy: "danger-full-access",
  permissionProfile: "disabled",
  cwd: "C:/work",
  workspaceRoots: ["C:/work"],
  userPrompt: "What controls?",
  finalAnswer: "The executed amendment controls.",
  durableCompletion: {
    runId: "run-1",
    revision: 2,
    omittedChecklistItems: 0,
    checklist: [],
    coverageText: "The executed amendment controls.",
  },
  usage: tokens,
  modelResponses: 2,
  calls: { readBearingToolCalls: 1 },
});

test("requires semantic coverage in the durable completion when registered", () => {
  const manifest = {
    name: "durable series",
    maturationUsage: {
      totalTokens: 0,
      uncachedTotalTokens: 0,
      modelResponses: 0,
    },
    cases: [
      {
        id: "case",
        prompt: "What controls?",
        requireDurableCompletion: true,
        expectedConcepts: [
          { name: "authority", termGroups: [["executed"], ["controls"]] },
        ],
        forbiddenPhrases: ["proposal controls"],
      },
    ],
  };
  const stateful = summary("stateful", usage(60, 30));
  stateful.durableCompletion.coverageText = "The answer was completed.";

  const report = compareSeries(
    manifest,
    new Map([
      [
        "case",
        {
          baseline: summary("baseline", usage(80, 50)),
          stateful,
        },
      ],
    ]),
  );

  assert.equal(report.cases[0].baselineAnswer.passed, true);
  assert.equal(report.cases[0].statefulAnswer.passed, true);
  assert.equal(report.cases[0].statefulDurableCompletion.passed, false);
  assert.equal(report.passed, false);
});

test("keeps maturation cost in the aggregate lifetime result", () => {
  const manifest = {
    name: "series",
    maturationUsage: {
      totalTokens: 100,
      uncachedTotalTokens: 40,
      modelResponses: 3,
    },
    cases: [
      {
        id: "case",
        prompt: "What controls?",
        expectedConcepts: [
          { name: "authority", termGroups: [["executed"], ["controls"]] },
        ],
        forbiddenPhrases: ["proposal controls"],
      },
    ],
  };
  const report = compareSeries(
    manifest,
    new Map([
      [
        "case",
        {
          baseline: summary("baseline", usage(80, 50)),
          stateful: summary("stateful", usage(60, 30)),
        },
      ],
    ]),
  );

  assert.equal(report.passed, true);
  assert.deepEqual(report.aggregate.statefulLifetime, {
    totalTokens: 160,
    uncachedTotalTokens: 70,
    modelResponses: 5,
    readBearingToolCalls: 1,
  });
  assert.deepEqual(report.aggregate.projectedBreakEvenQuestions, {
    totalTokens: 5,
    uncachedTotalTokens: 2,
  });
});

test("accepts measured maturation usage without changing the frozen manifest", () => {
  const manifest = {
    name: "series",
    maturationUsage: {
      totalTokens: 100,
      uncachedTotalTokens: 40,
      modelResponses: 3,
    },
    cases: [
      {
        id: "case",
        prompt: "What controls?",
        expectedConcepts: [
          { name: "authority", termGroups: [["executed"], ["controls"]] },
        ],
        forbiddenPhrases: ["proposal controls"],
      },
    ],
  };
  const measured = {
    totalTokens: 25,
    uncachedTotalTokens: 10,
    modelResponses: 1,
  };

  const report = compareSeries(
    manifest,
    new Map([
      [
        "case",
        {
          baseline: summary("baseline", usage(80, 50)),
          stateful: summary("stateful", usage(60, 30)),
        },
      ],
    ]),
    measured,
  );

  assert.deepEqual(report.aggregate.maturation, measured);
  assert.deepEqual(report.aggregate.statefulLifetime, {
    totalTokens: 85,
    uncachedTotalTokens: 40,
    modelResponses: 3,
    readBearingToolCalls: 1,
  });
  assert.equal(manifest.maturationUsage.totalTokens, 100);
});
