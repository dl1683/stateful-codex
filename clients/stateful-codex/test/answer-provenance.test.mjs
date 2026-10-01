import assert from "node:assert/strict";
import test from "node:test";

import { findTurnMeasurement } from "../public/answer-provenance.mjs";
import { renderTrustLine } from "../public/workspace-view.mjs";
import { workspaceFixture } from "./workspace-fixture.mjs";

function measurement(turnId, counters = {}) {
  return {
    threadId: "thread-1",
    turnId,
    status: "completed",
    counters: {
      rootEntriesLoaded: 9,
      rootEvidenceRoutesStale: 2,
      rootEvidenceRoutesUnavailable: 0,
      evidenceReadCalls: 0,
      ...counters,
    },
  };
}

function answeredState(items = []) {
  const state = workspaceFixture();
  state.threadId = "thread-1";
  state.activity = [
    { turnId: "turn-4", item: { type: "userMessage", id: "u", content: [] } },
    ...items.map((item) => ({ turnId: "turn-4", item })),
    { turnId: "turn-4", item: { type: "agentMessage", id: "m", text: "Nothing is outstanding." } },
  ];
  state.answerMeasurement = measurement("turn-4");
  return state;
}

test("an answer that rested on changed saved sources without checking them is flagged", () => {
  assert.equal(
    renderTrustLine(answeredState()),
    '<p class="trust-line warn"><strong>Latest answer:</strong> used saved project memory · some of its saved sources had changed since they were saved · 0 exact source reads · 0 commands run · 0 patches applied. No source reads, commands or other tools were recorded for it, so parts of it may be out of date; ask it to check the files.</p>',
  );
});

test("only actions that happened are counted, and other tools are never hidden", () => {
  const state = answeredState([
    { type: "commandExecution", id: "c1", status: "completed", command: "git status" },
    { type: "commandExecution", id: "c2", status: "declined", command: "rm -r build" },
    { type: "commandExecution", id: "c3", status: "failed", command: "pytest", exitCode: 1 },
    { type: "commandExecution", id: "c4", status: "failed", command: "curl x", exitCode: null },
    { type: "fileChange", id: "f1", status: "declined", changes: [] },
    { type: "mcpToolCall", id: "t1", tool: "fetch", status: "completed" },
  ]);

  assert.equal(
    renderTrustLine(state),
    '<p class="trust-line"><strong>Latest answer:</strong> used saved project memory · some of its saved sources had changed since they were saved · 0 exact source reads · 2 commands run · 3 actions declined or not run · 0 patches applied · 1 other tool call.</p>',
  );

  // A bounded item page that starts inside the turn makes the counts lower bounds.
  state.activityTruncated = true;
  assert.match(renderTrustLine(state), /at least 2 commands run · at least 3 actions declined or not run · at least 0 patches applied · at least 1 other tool call\.<\/p>$/);
});

test("the previous answer's provenance is hidden while a newer turn is under way", () => {
  const state = answeredState();
  state.liveTurnId = "turn-5";
  assert.equal(renderTrustLine(state), "");

  // Recorded activity can also show the newer turn before its answer exists.
  state.liveTurnId = null;
  state.activity.push({ turnId: "turn-5", item: { type: "userMessage", id: "u2", content: [] } });
  assert.equal(renderTrustLine(state), "");

  // A late live event from an older, recorded turn never hides the recorded answer.
  const late = answeredState();
  late.activity.unshift({ turnId: "turn-3", item: { type: "agentMessage", id: "old", text: "old" } });
  late.liveTurnId = "turn-3";
  assert.notEqual(renderTrustLine(late), "");

  // And a measurement for another turn never describes this answer.
  const other = answeredState();
  other.answerMeasurement = measurement("turn-3");
  assert.equal(renderTrustLine(other), "");
});

test("the answer's measurement is found beyond newer measurements from other threads", async () => {
  const pages = [
    { data: Array.from({ length: 50 }, (_, index) => ({ ...measurement(`x${index}`), threadId: "other" })), nextCursor: "p2" },
    { data: [measurement("turn-3"), measurement("turn-4")], nextCursor: "p3" },
  ];
  const cursors = [];
  const rpc = async (method, params) => {
    assert.equal(method, "statefulMeasurement/list");
    cursors.push(params.cursor);
    return pages[cursors.length - 1];
  };

  const found = await findTurnMeasurement(rpc, {
    projectId: "p",
    threadId: "thread-1",
    turnId: "turn-4",
    known: null,
  });
  assert.equal(found.turnId, "turn-4");
  assert.deepEqual(cursors, [null, "p2"]);

  // Once found it is kept, so later refreshes do not page again.
  const again = await findTurnMeasurement(rpc, {
    projectId: "p",
    threadId: "thread-1",
    turnId: "turn-4",
    known: found,
  });
  assert.equal(again, found);
  assert.equal(cursors.length, 2);
});

test("viewing an image counts as a tool, so the answer is not called unchecked", () => {
  const state = answeredState([{ type: "imageView", id: "i1", path: "C:/work/plot.png" }]);
  assert.match(renderTrustLine(state), /· 1 other tool call\.<\/p>$/);
  assert.doesNotMatch(renderTrustLine(state), /warn/);
});

test("an answer whose record is beyond the bounded search says so instead of vanishing", async () => {
  const page = {
    data: Array.from({ length: 50 }, (_, index) => ({ ...measurement(`x${index}`), threadId: "other" })),
    nextCursor: "more",
  };
  const found = await findTurnMeasurement(async () => page, {
    projectId: "p",
    threadId: "thread-1",
    turnId: "turn-4",
    known: null,
  });
  const state = answeredState();
  state.answerMeasurement = found;
  assert.equal(
    renderTrustLine(state),
    `<p class="trust-line">The latest answer's record is older than the recent turns loaded here, so what it rested on is not shown.</p>`,
  );
});
