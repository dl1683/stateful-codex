import assert from "node:assert/strict";
import test from "node:test";

import {
  armForLabel,
  redactArmPaths,
} from "../eval/prepare-blinded-grading.mjs";

test("assigns complementary deterministic blinded labels", () => {
  const first = armForLabel("project", "seed", "A");
  const second = armForLabel("project", "seed", "B");
  assert.deepEqual(new Set([first, second]), new Set(["baseline", "stateful"]));
  assert.equal(armForLabel("project", "seed", "A"), first);
});

test("redacts both arm-specific source roots", () => {
  const root = "C:\\snapshots";
  const value = [
    "C:\\snapshots\\project\\baseline\\a.md",
    "C:/snapshots/project/stateful/b.md",
  ].join("\n");
  assert.equal(
    redactArmPaths(value, root, "project"),
    "<PROJECT_ROOT>\\a.md\n<PROJECT_ROOT>/b.md",
  );
});
