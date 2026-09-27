import assert from "node:assert/strict";
import test from "node:test";

import {
  createRefreshGate,
  needsProjectRefresh,
} from "../public/refresh-policy.mjs";

test("startup retries missing and incomplete full refreshes", () => {
  assert.equal(needsProjectRefresh({ initialized: false }), true);
  assert.equal(
    needsProjectRefresh({ initialized: true, lastRefresh: null }),
    true,
  );
  assert.equal(
    needsProjectRefresh({
      initialized: true,
      lastRefresh: { inventoryComplete: false },
    }),
    true,
  );
  assert.equal(
    needsProjectRefresh({
      initialized: true,
      lastRefresh: {
        inventoryComplete: true,
        regionCoverageComplete: false,
      },
    }),
    false,
  );
});

test("refresh gate replays an event that arrives during an active refresh", async () => {
  const releases = [];
  let calls = 0;
  let markSecondStarted;
  const secondStarted = new Promise((resolve) => {
    markSecondStarted = resolve;
  });
  const refresh = createRefreshGate(async () => {
    calls += 1;
    if (calls === 2) {
      markSecondStarted();
    }
    await new Promise((resolve) => releases.push(resolve));
  });

  const active = refresh();
  assert.equal(calls, 1);
  assert.equal(refresh(), active);

  releases.shift()();
  await secondStarted;
  assert.equal(calls, 2);

  releases.shift()();
  await active;
  assert.equal(calls, 2);
});
