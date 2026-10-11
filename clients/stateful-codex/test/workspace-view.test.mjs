import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  applyWorkspaceEvent,
  reconcileAskStream,
} from "../public/workspace-events.mjs";
import { askAnswer, renderWorkspace } from "../public/workspace-view.mjs";
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

test("an Ask workspace shows the answer with no run controls", () => {
  const state = workspaceFixture();
  state.ask = true;
  state.run = null;
  state.obligations = [];
  state.steering = [];
  state.liveText = "";
  state.activity = [
    {
      turnId: "turn-1",
      item: {
        type: "agentMessage",
        id: "message-1",
        text: "We chose SQLite over Postgres for offline laptop use.",
      },
    },
  ];

  const actual = renderWorkspace(state);

  assert.match(actual, /<span class="badge ask">ask · no run<\/span>/);
  assert.match(actual, /<h2>Answer<\/h2>/);
  assert.match(actual, /We chose SQLite over Postgres for offline laptop use\./);
  assert.match(actual, /id="message-form"/);
  for (const absent of [
    /id="steering-form"/,
    /id="mode-form"/,
    /data-action="cancel"/,
    /Current obligation/,
    /continuations/,
    /blocked/i,
  ]) {
    assert.doesNotMatch(actual, absent);
  }
});

function askState() {
  const state = workspaceFixture();
  state.threadId = "thread-1";
  state.ask = true;
  state.run = null;
  state.obligations = [];
  state.steering = [];
  state.liveText = "";
  state.liveItemId = null;
  state.liveItemText = "";
  state.completedAnswer = null;
  state.activity = [
    {
      turnId: "turn-0",
      item: { type: "agentMessage", id: "earlier", text: "An earlier answer." },
    },
  ];
  return state;
}

const askDelta = (text) => ({
  method: "item/agentMessage/delta",
  params: { threadId: "thread-1", turnId: "turn-1", itemId: "answer-1", delta: text },
});

const askCompleted = (text) => ({
  method: "item/completed",
  params: {
    threadId: "thread-1",
    turnId: "turn-1",
    item: { type: "agentMessage", id: "answer-1", text },
  },
});

test("a reload mid-answer shows the complete answer once it completes", () => {
  // After a reload the new subscription only sees the tail of the stream.
  const state = askState();
  applyWorkspaceEvent(state, askDelta("is."));
  assert.equal(askAnswer(state), "is.");
  applyWorkspaceEvent(state, askCompleted("Paris."));
  assert.equal(askAnswer(state), "Paris.");
  // A later refresh that lists the item keeps the same authoritative text.
  state.activity.push({ turnId: "turn-1", item: { type: "agentMessage", id: "answer-1", text: "Paris." } });
  assert.equal(askAnswer(state), "Paris.");
  assert.match(renderWorkspace(state), /<p class="live-copy">Paris\.<\/p>/);
});

test("lost middle deltas are repaired by the completed item", () => {
  const state = askState();
  for (const piece of ["The capital ", "France "]) applyWorkspaceEvent(state, askDelta(piece));
  applyWorkspaceEvent(state, askCompleted("The capital of France is Paris."));
  assert.equal(askAnswer(state), "The capital of France is Paris.");
});

test("a completion missed while disconnected is reconciled on reconnect", () => {
  const state = askState();
  applyWorkspaceEvent(state, askDelta("The capital "));
  // The connection drops; the answer completes unseen; the gateway reconnects.
  const effect = applyWorkspaceEvent(state, { method: "gateway/connected", params: {} });
  assert.equal(effect.refresh, true);
  // The refresh lists the thread's completed items.
  state.activity.push({
    turnId: "turn-1",
    item: { type: "agentMessage", id: "answer-1", text: "The capital of France is Paris." },
  });
  reconcileAskStream(state);
  assert.equal(askAnswer(state), "The capital of France is Paris.");
});

const agentItem = (turnId, id, text) => ({ turnId, item: { type: "agentMessage", id, text } });
const event = (method, turnId, item) => ({
  method,
  params: { threadId: "thread-1", turnId, item: { type: "agentMessage", ...item } },
});
const streamed = (turnId, itemId, delta) => ({
  method: "item/agentMessage/delta",
  params: { threadId: "thread-1", turnId, itemId, delta },
});
const answerPanel = (state) =>
  renderWorkspace(state).match(/<h2>Answer<\/h2>.*?<\/section>/s)[0];

test("a final answer whose deltas were all missed replaces the commentary", () => {
  const state = askState();
  applyWorkspaceEvent(state, event("item/started", "turn-1", { id: "comment", text: "" }));
  applyWorkspaceEvent(state, streamed("turn-1", "comment", "Let me check the source."));
  applyWorkspaceEvent(state, event("item/completed", "turn-1", { id: "comment", text: "Let me check the source." }));
  // The connection drops during the final message's deltas and returns before its completion.
  assert.equal(applyWorkspaceEvent(state, { method: "gateway/connected", params: {} }).refresh, true);
  state.activity.push(agentItem("turn-1", "comment", "Let me check the source."));
  reconcileAskStream(state);
  applyWorkspaceEvent(state, event("item/completed", "turn-1", { id: "final", text: "Paris." }));
  assert.equal(askAnswer(state), "Paris.");
  assert.match(answerPanel(state), /<p class="live-copy">Paris\.<\/p>/);
  // The refresh after completion lists both; the latest completed answer still wins.
  state.activity.push(agentItem("turn-1", "final", "Paris."));
  reconcileAskStream(state);
  assert.equal(askAnswer(state), "Paris.");
  assert.match(answerPanel(state), /<p class="live-copy">Paris\.<\/p>/);
});

test("a later turn missed entirely while disconnected shows its completed answer", () => {
  const state = askState();
  applyWorkspaceEvent(state, streamed("turn-1", "first", "The capital of France"));
  // Disconnected: turn-1 completes and a whole later turn runs unseen; then the gateway returns.
  applyWorkspaceEvent(state, { method: "gateway/connected", params: {} });
  state.activity.push(
    agentItem("turn-1", "first", "The capital of France is Paris."),
    agentItem("turn-2", "second", "A leap year has 366 days."),
  );
  reconcileAskStream(state);
  assert.equal(askAnswer(state), "A leap year has 366 days.");
  assert.match(answerPanel(state), /<p class="live-copy">A leap year has 366 days\.<\/p>/);
  // A genuinely newer stream that the history does not list yet is kept.
  applyWorkspaceEvent(state, streamed("turn-3", "third", "Madrid"));
  reconcileAskStream(state);
  assert.equal(askAnswer(state), "Madrid");
});

test("an old cached answer outside the history page never hides a newer one", () => {
  const state = askState();
  applyWorkspaceEvent(state, event("item/completed", "turn-1", { id: "old", text: "Paris." }));
  assert.equal(askAnswer(state), "Paris.");
  // Disconnected: a tool-heavy later turn produces 100+ items and completes a new answer unseen.
  applyWorkspaceEvent(state, { method: "gateway/connected", params: {} });
  const snapshotSeq = state.eventSeq;
  state.activity = [
    ...Array.from({ length: 99 }, (_, index) => ({
      turnId: "turn-2",
      item: { type: "commandExecution", id: `tool-${index}`, status: "completed", command: "rg x" },
    })),
    agentItem("turn-2", "new", "The source says Madrid."),
  ];
  reconcileAskStream(state, snapshotSeq);
  assert.equal(askAnswer(state), "The source says Madrid.");
  assert.match(answerPanel(state), /<p class="live-copy">The source says Madrid\.<\/p>/);
});

test("a completion that arrives during a history read is kept", () => {
  const state = askState();
  applyWorkspaceEvent(state, event("item/completed", "turn-1", { id: "first", text: "Paris." }));
  // A refresh issues its history read; before it returns, a newer answer completes.
  const snapshotSeq = state.eventSeq;
  applyWorkspaceEvent(state, event("item/completed", "turn-2", { id: "second", text: "Madrid." }));
  // The read returns the history as of its request: it lists only the first answer.
  state.activity = [agentItem("turn-1", "first", "Paris.")];
  reconcileAskStream(state, snapshotSeq);
  assert.equal(askAnswer(state), "Madrid.");
  assert.match(answerPanel(state), /<p class="live-copy">Madrid\.<\/p>/);
  // The next refresh lists it; the history now carries the newest answer itself.
  state.activity.push(agentItem("turn-2", "second", "Madrid."));
  reconcileAskStream(state);
  assert.equal(askAnswer(state), "Madrid.");
});

test("a refused Ask shows the refusal and no composer", () => {
  const state = askState();
  state.askRefusal = "This thread has an open autonomous run, so a question here would join it.";
  const actual = renderWorkspace(state);
  assert.match(actual, /<h2>Ask refused<\/h2>/);
  assert.match(actual, /ask · refused: open run/);
  assert.doesNotMatch(actual, /id="message-form"/);
  assert.doesNotMatch(actual, /ask · no run/);
});

function statefulFindingCount(state) {
  return state.blackboard.length;
}
