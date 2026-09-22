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
  usage: tokens,
  modelResponses: 2,
  calls: { readBearingToolCalls: 1 },
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
