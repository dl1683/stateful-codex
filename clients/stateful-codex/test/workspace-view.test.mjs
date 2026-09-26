import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { renderWorkspace } from "../public/workspace-view.mjs";
import { workspaceFixture } from "./workspace-fixture.mjs";

test("workspace presents semantic progress and evidence before raw activity", async () => {
  const actual = renderWorkspace(workspaceFixture());
  const expected = await readFile(
    new URL("snapshots/workspace.html", import.meta.url),
    "utf8",
  );
  assert.equal(`${actual.trim()}\n`, expected);
  assert.ok(
    actual.indexOf("Current obligation") <
      actual.indexOf("Supporting activity"),
  );
  assert.match(actual, /A result is not automatically verified/);
  assert.match(actual, /Command/);
  assert.equal(
    actual.match(/data-action="confirm-knowledge"/g)?.length,
    statefulFindingCount(workspaceFixture()),
  );
  assert.doesNotMatch(actual, /undefined · Recorded/);
});

test("already user-confirmed understanding cannot be confirmed again", () => {
  const state = workspaceFixture();
  state.blackboard[0].effectiveVerification = "userConfirmed";

  const actual = renderWorkspace(state);

  assert.equal(
    actual.match(/data-action="confirm-knowledge"/g)?.length,
    statefulFindingCount(state) - 1,
  );
  assert.match(actual, /badge userConfirmed/);
});

test("terminal workspace preserves the record without accepting dead controls", () => {
  const state = workspaceFixture();
  state.run.status = "completed";

  const actual = renderWorkspace(state);

  assert.doesNotMatch(actual, /id="steering-form"/);
  assert.doesNotMatch(actual, /id="mode-form"/);
  assert.doesNotMatch(actual, /id="message-form"/);
  assert.doesNotMatch(actual, /data-action="maintain"/);
  assert.match(actual, /Start another outcome/);
});

function statefulFindingCount(state) {
  return state.blackboard.length;
}
