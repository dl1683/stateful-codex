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

  assert.match(actual, /aria-label="model responses unavailable"><strong aria-hidden="true">—<\/strong>/);
  assert.match(
    actual,
    /Model-response, model-tool, and tool-output totals are unavailable/,
  );
  assert.match(actual, /Model tool calls and tool-output bytes unavailable/);
  assert.doesNotMatch(actual, /aria-label="0 model responses"/);
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

test("intelligence counts are labelled by the population each one measures", () => {
  const state = workspaceFixture();
  state.status = {
    ...state.status,
    blackboardEntryCount: 66,
    contextMapEntryCount: 21135,
    fileCount: 2864,
    missingSourceCount: 3,
    lastRefresh: {
      ...state.status.lastRefresh,
      filesIndexed: 2864,
      regionsIndexed: 17950,
      completedAt: 1790000000,
    },
  };

  const tiles = metricTiles(renderWorkspace(state), "Project intelligence");

  assert.deepEqual(tiles, [
    ["66 saved understandings (blackboard entries)", "66", "Saved understandings · blackboard entries"],
    ["21,135 source index entries (file and region routes)", "21,135", "Source index entries · file and region routes"],
    ["2,864 files mapped", "2,864", "Files mapped"],
    ["3 missing sources", "3", "Missing sources"],
    [
      "17,950 indexed source regions at last refresh (2026-09-21 14:13 UTC)",
      "17,950",
      "Indexed source regions at last refresh · 2026-09-21 14:13 UTC",
    ],
  ]);
});

test("region counts carry the last refresh's completeness and are never inferred", () => {
  const state = workspaceFixture();
  state.status.lastRefresh.regionCoverageComplete = false;
  assert.equal(
    metricTiles(renderWorkspace(state), "Project intelligence")[4][0],
    "9 indexed source regions at last refresh (2026-09-21 14:13 UTC · region coverage partial)",
  );

  state.status.lastRefresh = null;
  const tiles = metricTiles(renderWorkspace(state), "Project intelligence");
  assert.deepEqual(tiles[4], [
    "indexed source regions at last refresh unavailable (no completed refresh recorded)",
    "—",
    "Indexed source regions at last refresh · no completed refresh recorded",
  ]);
  assert.equal(tiles[1][1], "18");
});

// [accessible name, visible value, visible label] for each metric tile in one panel.
function metricTiles(html, panelTitle) {
  const panel = html.split("<h2>").find((part) => part.startsWith(panelTitle));
  return [
    ...panel.matchAll(
      /role="group" aria-label="([^"]*)"><strong aria-hidden="true">([^<]*)<\/strong><span aria-hidden="true">([^<]*)<\/span>/g,
    ),
  ].map((match) => match.slice(1));
}

test("completed agent messages stay readable outside the bounded live tail", () => {
  const state = workspaceFixture();
  state.activity.push({
    turnId: "turn-4",
    item: { type: "agentMessage", id: "message-1", text: "The full <final> answer." },
  });

  const actual = renderWorkspace(state);

  assert.match(
    actual,
    /<details class="workspace-panel" id="recorded-messages"><summary>Recorded agent messages · 1 recent<\/summary><p class="recorded-message">The full &lt;final&gt; answer\.<\/p><\/details>/,
  );
  assert.match(actual, /Supporting activity · 1 recent items/);
});

function statefulFindingCount(state) {
  return state.blackboard.length;
}
