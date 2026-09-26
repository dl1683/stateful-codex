import assert from "node:assert/strict";
import test from "node:test";

import { needsProjectRefresh } from "../public/refresh-policy.mjs";

test("startup retries missing and incomplete full refreshes", () => {
  assert.equal(needsProjectRefresh({ initialized: false }), true);
  assert.equal(needsProjectRefresh({ initialized: true, lastRefresh: null }), true);
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
