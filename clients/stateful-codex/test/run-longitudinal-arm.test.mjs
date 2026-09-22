import assert from "node:assert/strict";
import test from "node:test";

import {
  nextAttemptNumber,
  unresolvedAttemptForCase,
} from "../eval/run-longitudinal-arm.mjs";

test("numbers attempts without overwriting earlier evidence", () => {
  const state = {
    attempts: [
      { caseId: "q01", number: 1, status: "completed" },
      { caseId: "q02", number: 1, status: "completed" },
      { caseId: "q01", number: 2, status: "completed" },
    ],
  };
  assert.equal(nextAttemptNumber(state, "q01"), 3);
  assert.equal(nextAttemptNumber(state, "q03"), 1);
});

test("requires reconciliation after a failed or interrupted attempt", () => {
  const failed = { caseId: "q01", number: 1, status: "failed" };
  const state = {
    attempts: [
      { caseId: "q00", number: 1, status: "completed" },
      failed,
    ],
  };
  assert.equal(unresolvedAttemptForCase(state, "q01"), failed);
  assert.equal(unresolvedAttemptForCase(state, "q00"), undefined);
});
