import assert from "node:assert/strict";
import test from "node:test";

import {
  armForLabel,
  gradingOutputsOverlap,
  redactArmPaths,
} from "../eval/prepare-blinded-grading.mjs";

test("assigns complementary deterministic blinded labels", () => {
  const first = armForLabel("project", "seed", "A");
  const second = armForLabel("project", "seed", "B");
  assert.deepEqual(new Set([first, second]), new Set(["baseline", "stateful"]));
  assert.equal(armForLabel("project", "seed", "A"), first);
});

test("requires public packets and private mappings to use disjoint directories", () => {
  assert.equal(gradingOutputsOverlap("grading/public", "grading/private"), false);
  assert.equal(gradingOutputsOverlap("grading", "grading/private"), true);
  assert.equal(gradingOutputsOverlap("grading/public", "grading"), true);
  assert.equal(gradingOutputsOverlap("grading", "grading"), true);
});

test("redacts both arm-specific source roots", () => {
  const root = "C:\\snapshots";
  const value = [
    "C:\\snapshots\\project\\baseline\\a.md",
    "C:/snapshots/project/stateful/b.md",
  ].join("\n");
  assert.equal(
    redactArmPaths(value, root, "project"),
    "PROJECT_ROOT\\a.md\nPROJECT_ROOT/b.md",
  );
});

test("redacts arm paths even when an answer misspells the project directory", () => {
  const value = "<C:\\snapshots\\project-alias\\stateful\\result.json:10>";
  assert.equal(
    redactArmPaths(value, "C:\\snapshots", "project",),
    "<PROJECT_ROOT\\result.json:10>",
  );
});

test("redacts absolute arm paths even when the snapshot directory is hallucinated", () => {
  const value =
    "<C:\\wrong-snapshot\\project\\stateful\\result.json:10> and C:/other/project/baseline/a.md";
  assert.equal(
    redactArmPaths(value, "C:\\snapshots", "project"),
    "<PROJECT_ROOT\\result.json:10> and PROJECT_ROOT/a.md",
  );
});
