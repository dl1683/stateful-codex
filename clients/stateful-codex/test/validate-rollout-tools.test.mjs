import assert from "node:assert/strict";
import test from "node:test";

import { validateRolloutToolAssertions } from "../eval/validate-rollout-tools.mjs";

const benchmarkCase = {
  id: "reconcile",
  prompt: "Explain the complete decision history.",
  statefulToolAssertions: { blackboardEntryScopes: ["historical"] },
};

test("requires registered historical blackboard retrieval", () => {
  const result = validateRolloutToolAssertions({
    summary: summaryWithScopes(["active", "historical"]),
    benchmarkCase,
  });
  assert.deepEqual(result, {
    blackboardEntryScopes: ["active", "historical"],
    requiredBlackboardEntryScopes: ["historical"],
  });
  assert.throws(
    () =>
      validateRolloutToolAssertions({
        summary: summaryWithScopes(["active"]),
        benchmarkCase,
      }),
    /did not query required blackboard entry scopes: historical/,
  );
});

function summaryWithScopes(scopes) {
  return {
    turns: [
      {
        userPrompt: benchmarkCase.prompt,
        calls: { blackboardEntryScopes: scopes },
      },
    ],
  };
}
