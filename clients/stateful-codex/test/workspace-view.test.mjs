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
  assert.match(actual, /Terminal coverage 3\/4/);
  assert.match(actual, /Recorded trajectory subtotal covers 3\/4 records/);
  assert.match(actual, /Older measurements exist outside this window/);
  assert.match(actual, /no monetary cost is inferred/);
  assert.match(actual, /Command/);
  assert.equal(
    actual.match(/data-action="confirm-knowledge"/g)?.length,
    statefulFindingCount(workspaceFixture()),
  );
  assert.doesNotMatch(actual, /undefined · Recorded/);
});

test("missing terminal trajectory is not rendered as measured zeroes", () => {
  const state = workspaceFixture();
  state.measurementSummary.terminalMeasurementCount = 0;
  state.measurementSummary.completedTurns = 0;
  state.measurementSummary.failedTurns = 0;
  state.measurementSummary.abortedTurns = 0;
  state.measurementSummary.trajectory = null;

  const actual = renderWorkspace(state);

  assert.match(actual, /<strong>—<\/strong><span>model responses<\/span>/);
  assert.match(
    actual,
    /Model-response, model-tool, and tool-output totals are unavailable/,
  );
  assert.match(actual, /Model tool calls and tool-output bytes unavailable/);
  assert.doesNotMatch(
    actual,
    /<strong>0<\/strong><span>model responses<\/span>/,
  );
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

test("workspace warns when the durable file inventory is incomplete", () => {
  const state = workspaceFixture();
  state.status.lastRefresh.inventoryComplete = false;
  state.status.lastRefresh.filesSkipped = 2;

  const actual = renderWorkspace(state);

  assert.match(actual, /File inventory incomplete · 2 skipped/);
  assert.match(actual, /unindexed files must not be treated as absent/);
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

test("an answered run reads as a calm finished answer", () => {
  const state = workspaceFixture();
  state.run.status = "answered";
  state.run.result = "parse_config returns the parsed Config.";

  const actual = renderWorkspace(state);

  assert.match(
    actual,
    /<span class="badge answered">answered · not verified<\/span>/,
  );
  assert.match(actual, /<h2>Answer<\/h2>/);
  assert.match(actual, /The agent's answer, not verified by the host\./);
  // The run's own status and answer carry no warning or error styling and no Blocked wording.
  const summary = actual.match(/<div class="run-summary">.*?<\/div>/)[0];
  const answer = actual.match(/<h2>Answer<\/h2>.*?<\/section>/)[0];
  for (const part of [summary, answer]) {
    assert.doesNotMatch(part, /blocked|failed|warn|error|danger/i);
    assert.doesNotMatch(part, /class="badge (pending|unverified|stale)"/);
  }
  assert.doesNotMatch(actual, /id="steering-form"/);
  assert.doesNotMatch(actual, /id="message-form"/);
  assert.doesNotMatch(actual, /data-action="cancel"/);
});

function statefulFindingCount(state) {
  return state.blackboard.length;
}
