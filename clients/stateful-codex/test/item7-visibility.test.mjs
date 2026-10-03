import assert from "node:assert/strict";
import test from "node:test";

import {
  applySnapshot,
  beginSnapshot,
  createExecution,
  noteConnectionLost,
  snapshotPhase,
} from "../public/execution-state.mjs";
import { admitRpcMethod } from "../gateway-policy.mjs";
import { runStateLabel } from "../public/memory-view.mjs";
import { clipText, groupReceipt, knowledgeReceipt } from "../public/memory-receipts.mjs";
import { memoryStatusLine, renderRecap } from "../public/memory-status.mjs";
import { createSummaryReader, newerSummary } from "../public/status-reads.mjs";
import { createWorkspaceDom } from "../public/workspace-dom.mjs";
import { applyWorkspaceEvent } from "../public/workspace-events.mjs";
import { renderWorkspace } from "../public/workspace-view.mjs";
import { createDocument } from "./mini-dom.mjs";
import { workspaceFixture } from "./workspace-fixture.mjs";

const RUN = { id: "run-1", mode: "collaborative", status: "running" };
const ACTIVE = { status: { type: "active", activeFlags: [] } };
const IDLE = { status: { type: "idle" } };

function liveState() {
  return {
    threadId: "thread-a",
    projectId: "project-1",
    run: RUN,
    pendingRequests: [],
    execution: createExecution(),
  };
}

test("a fresh page says it is checking, never Idle, before anything authoritative", () => {
  const state = liveState();
  assert.equal(runStateLabel(state), "Checking what is running… · Collaborative run stays open between answers");
  assert.doesNotMatch(runStateLabel(state), /Idle/);
});

test("reload mid-turn without the started event shows Working from thread/read", () => {
  const state = liveState();
  // The page was reloaded after turn/started was sent: only the snapshot can tell.
  const token = beginSnapshot(state.execution);
  applySnapshot(state.execution, token, {
    thread: ACTIVE,
    latestTurn: { id: "t1", status: "inProgress" },
  });
  assert.equal(runStateLabel(state), "Working · Collaborative run stays open between answers");
  // The turn then completes live.
  applyWorkspaceEvent(state, {
    method: "turn/completed",
    params: { threadId: "thread-a", turn: { id: "t1", status: "completed" } },
  });
  assert.equal(state.execution.phase, "finished");
});

test("a snapshot older than a live event never overwrites it", () => {
  const state = liveState();
  const token = beginSnapshot(state.execution);
  // While the read is in flight the turn starts.
  applyWorkspaceEvent(state, {
    method: "turn/started",
    params: { threadId: "thread-a", turn: { id: "t2" } },
  });
  // The read was taken before the start and says idle with the last turn finished.
  const applied = applySnapshot(state.execution, token, {
    thread: IDLE,
    latestTurn: { id: "t1", status: "completed" },
  });
  assert.equal(applied, false);
  assert.equal(state.execution.phase, "working");
  // An older read finishing after a newer one is discarded too.
  const first = beginSnapshot(state.execution);
  const second = beginSnapshot(state.execution);
  applySnapshot(state.execution, second, { thread: ACTIVE, latestTurn: null });
  assert.equal(applySnapshot(state.execution, first, { thread: IDLE, latestTurn: null }), false);
});

test("a dropped connection makes the state unknown until the next read", () => {
  const state = liveState();
  applySnapshot(state.execution, beginSnapshot(state.execution), { thread: IDLE, latestTurn: null });
  assert.equal(state.execution.phase, "idle");
  applyWorkspaceEvent(state, { method: "gateway/error", params: { message: "Live connection was interrupted." } });
  assert.match(runStateLabel(state), /^Checking what is running…/);
  const effect = applyWorkspaceEvent(state, {
    method: "gateway/pendingRequests",
    params: { threadId: "thread-a", requests: [] },
  });
  assert.equal(effect.refresh, true, "a reconnect re-reads the authoritative state");
});

test("snapshot phases are truthful about what they cannot know", () => {
  assert.deepEqual(
    [
      snapshotPhase(null, null),
      snapshotPhase(ACTIVE, undefined),
      snapshotPhase({ status: { type: "active", activeFlags: ["waitingOnApproval"] } }, null),
      snapshotPhase(IDLE, { status: "completed" }),
      snapshotPhase(IDLE, { status: "interrupted" }),
      snapshotPhase(IDLE, { status: "failed" }),
      snapshotPhase(IDLE, null),
      snapshotPhase(IDLE, undefined),
      snapshotPhase({ status: { type: "notLoaded" } }, { status: "inProgress" }),
    ],
    ["unknown", "working", "waiting", "finished", "stopped", "failed", "idle", "unknown", "unknown"],
  );
});

test("an approval snapshot replaces the waiting list and takes priority", () => {
  const state = liveState();
  applySnapshot(state.execution, beginSnapshot(state.execution), { thread: ACTIVE, latestTurn: null });
  const approval = {
    id: 7,
    method: "item/commandExecution/requestApproval",
    params: { threadId: "thread-a", turnId: "t", itemId: "i", startedAtMs: 1 },
  };
  applyWorkspaceEvent(state, approval);
  assert.match(runStateLabel(state), /^Waiting for you/);
  // Answered in another view while this one was away: the snapshot on reconnect clears it.
  applyWorkspaceEvent(state, {
    method: "gateway/pendingRequests",
    params: { threadId: "thread-a", requests: [] },
  });
  assert.deepEqual(state.pendingRequests, []);
  // A reconnect may have missed events: no claim until the next read.
  assert.match(runStateLabel(state), /^Checking what is running…/);
});

test("a remembered commit is said plainly, from workspace history", () => {
  const line = groupReceipt({
    category: "commit",
    saved: 1,
    pending: 0,
    omitted: 0,
    failed: 0,
    alreadyPresent: 0,
    items: [{ outcome: "stored", category: "commit", text: "1a2b3c4d Draft sections 4–7" }],
  });
  assert.equal(line, "Remembered commit 1a2b3c4d: Draft sections 4–7 · from workspace history");
  assert.doesNotMatch(line, /you committed|origin unknown/i);
  const many = groupReceipt({
    category: "commit",
    saved: 5,
    items: ["a1 One", "b2 Two", "c3 Three", "d4 Four", "e5 Five"].map((text) => ({ outcome: "stored", text })),
  });
  assert.equal(many, "Remembered 5 commits from workspace history: a1 One; b2 Two; c3 Three; and 2 more");
  assert.equal(groupReceipt({ category: "commit", saved: 0, items: [] }), null);
});

test("a counted rule group reports what was committed", () => {
  const line = groupReceipt({
    category: "rule",
    saved: 2,
    pending: 0,
    alreadyPresent: 1,
    omitted: 1,
    failed: 0,
    items: [
      { outcome: "stored", text: "Quote § numbers exactly." },
      { outcome: "stored", text: "Use British spelling." },
      { outcome: "alreadyStored", text: "Keep it short." },
    ],
  });
  assert.equal(
    line,
    'Saved 2 rules, 1 already saved, 1 too long to keep whole, not saved: "Quote § numbers exactly."; "Use British spelling."',
  );
});

test("group receipts reach the notices", () => {
  const state = liveState();
  const effect = applyWorkspaceEvent(state, {
    method: "statefulKnowledge/groupCaptured",
    params: {
      threadId: "thread-a",
      projectId: "project-1",
      category: "commit",
      saved: 1,
      items: [{ outcome: "stored", text: "9f8e7d6c Revise after editor feedback" }],
    },
  });
  assert.deepEqual(effect, { sections: ["notices"], refresh: true });
  assert.deepEqual(state.receipts, [
    "Remembered commit 9f8e7d6c: Revise after editor feedback · from workspace history",
  ]);
});

test("the memory status line counts what is held and what this session changed", () => {
  assert.equal(memoryStatusLine(null), "Memory: checking…");
  assert.equal(memoryStatusLine({ error: "nope" }), "Memory status unavailable");
  assert.equal(memoryStatusLine({ counts: {}, since: null }), "Memory: nothing saved yet");
  assert.equal(
    memoryStatusLine({
      counts: { rules: 2, decisions: 1, openChecks: 1, commits: 1 },
      since: { saved: 1, commitsRemembered: 2, forgotten: 1, captureIncomplete: 1 },
    }),
    "Memory: 2 rules · 1 decision · 1 open check · 1 remembered commit · this session: saved 1, 2 commits remembered, forgot 1, 1 capture could not finish",
  );
});

test("the session watermark is the first read's journal sequence", async () => {
  const storage = new Map();
  const store = { getItem: (key) => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value) };
  const calls = [];
  const rpc = async (method, params) => {
    calls.push(params.sinceSequence);
    return { counts: {}, latestSequence: 41 + calls.length, since: params.sinceSequence === null ? null : { saved: 1 } };
  };
  const read = createSummaryReader(rpc, { threadId: "thread-a", storage: store });
  const first = await read();
  assert.equal(first.since.saved, 0, "the first read starts the session with nothing changed");
  await read();
  // A reload of the tab continues the same session from storage.
  await createSummaryReader(rpc, { threadId: "thread-a", storage: store })();
  assert.deepEqual(calls, [null, 42, 42]);
  const failed = await createSummaryReader(async () => {
    throw new Error("method not found");
  }, { threadId: "thread-a", storage: store })();
  assert.deepEqual(failed, { error: "method not found" });
});

test("without working storage the page still keeps its own session watermark", async () => {
  for (const storage of [
    undefined,
    { getItem: () => { throw new Error("blocked"); }, setItem: () => { throw new Error("blocked"); } },
    { getItem: () => null, setItem: () => { throw new Error("full"); } },
  ]) {
    const calls = [];
    let sequence = 10;
    const read = createSummaryReader(async (method, params) => {
      calls.push(params.sinceSequence);
      sequence += 5;
      return { counts: {}, latestSequence: sequence, since: null };
    }, { threadId: "thread-a", storage });
    await read();
    await read();
    assert.deepEqual(calls, [null, 15]);
  }
});

test("a reconnect fences reads begun during the gap", () => {
  const state = liveState();
  applyWorkspaceEvent(state, { method: "gateway/error", params: { message: "interrupted" } });
  const duringGap = beginSnapshot(state.execution);
  applyWorkspaceEvent(state, { method: "gateway/pendingRequests", params: { threadId: "thread-a", requests: [] } });
  assert.equal(applySnapshot(state.execution, duringGap, { thread: IDLE, latestTurn: null }), false);
  assert.equal(state.execution.phase, "checking");
  const afterReconnect = beginSnapshot(state.execution);
  assert.equal(applySnapshot(state.execution, afterReconnect, { thread: IDLE, latestTurn: null }), true);
  assert.equal(state.execution.phase, "idle");
  noteConnectionLost(state.execution);
  assert.equal(state.execution.phase, "checking");
});

test("thread status changes are authoritative and fence older reads", () => {
  for (const [status, phase] of [
    [{ type: "active", activeFlags: ["waitingOnApproval"] }, "waiting"],
    [{ type: "active", activeFlags: [] }, "working"],
    [{ type: "systemError" }, "error"],
  ]) {
    const state = liveState();
    const token = beginSnapshot(state.execution);
    const effect = applyWorkspaceEvent(state, {
      method: "thread/status/changed",
      params: { threadId: "thread-a", status },
    });
    assert.ok(effect.sections.includes("header"));
    assert.equal(applySnapshot(state.execution, token, { thread: IDLE, latestTurn: null }), false);
    assert.equal(state.execution.phase, phase);
  }
  // Idle after work says the turn ended but not how: check rather than claim.
  const state = liveState();
  applySnapshot(state.execution, beginSnapshot(state.execution), { thread: ACTIVE, latestTurn: null });
  applyWorkspaceEvent(state, { method: "thread/status/changed", params: { threadId: "thread-a", status: { type: "idle" } } });
  assert.equal(state.execution.phase, "checking");
});

test("memory holding only unverified rules or other entries is not called empty", () => {
  assert.equal(
    memoryStatusLine({ counts: { other: 1, unverifiedRules: 1 } }),
    "Memory: 1 rule not in your words (not applied) · 1 other entry",
  );
});

test("the return card is dated, bounded, and invents no next step", () => {
  const html = renderRecap({ recap: workspaceFixture().recap });
  assert.match(html, /Where things stand, as of 2026-09-21 15:13 UTC/);
  assert.match(html, /Last finished 2026-09-21 14:13 UTC/);
  assert.match(html, /Because: Clause 7 replaces the 40% threshold — the agreement says so\./);
  assert.match(html, /and 2 more/);
  assert.match(html, /No next step was recorded/);
  assert.equal(renderRecap({ recap: { asOf: 1, rules: [], decisions: [], openChecks: [], commits: [] } }), "");
  assert.equal(renderRecap({ recap: workspaceFixture().recap, recapDismissed: true }), "");
});

test("one conversation composer; steering is a collapsed run control", () => {
  const html = renderWorkspace(workspaceFixture());
  assert.equal(html.match(/<textarea/g)?.length, 2);
  assert.equal(html.match(/id="message-form"/g)?.length, 1);
  assert.match(html, /<details class="workspace-panel steering-panel"><summary><h2>Steer the run<\/h2>/);
  assert.doesNotMatch(html, /Add an instruction/);
});

test("the gateway admits the new read methods", () => {
  for (const method of ["statefulMemory/summary", "statefulMemory/recap", "thread/turns/list", "thread/read"]) {
    assert.equal(admitRpcMethod(method), true, method);
  }
});

const UNICODE = "Check § 4–7 — “Ananya’s” note, Zoë 👩‍💻";

test("section signs, dashes, curly quotes and names round-trip through clipping and the DOM", () => {
  assert.equal(clipText(UNICODE, 1_000), UNICODE);
  // Cutting never splits a surrogate pair: every kept character is a whole code point.
  const cut = clipText("§😀😀😀😀", 3);
  assert.equal(cut, "§😀…");
  assert.equal(new TextDecoder("utf-8", { fatal: true }).decode(new TextEncoder().encode(cut)), cut);
  assert.equal(
    knowledgeReceipt({ outcome: "stored", category: "rule", text: UNICODE }),
    `Saved your rule: "${UNICODE}"`,
  );

  const document = createDocument();
  const root = document.createElement("div");
  document.body.append(root);
  const state = {
    ...workspaceFixture(),
    threadId: "thread-a",
    projectId: "project-1",
    pendingRequests: [],
    requestItems: new Map(),
    receipts: [],
  };
  const view = createWorkspaceDom(root, { schedule: () => {} });
  view.update(state);
  applyWorkspaceEvent(state, {
    method: "statefulKnowledge/captured",
    params: { threadId: "thread-a", turnId: "t", category: "rule", outcome: "stored", text: UNICODE },
  });
  view.update(state, ["notices"]);
  assert.equal(root.querySelector(".receipt").textContent, `Saved your rule: "${UNICODE}"`);
  assert.match(root.querySelector("[data-recap]").textContent, /Check § 7 of the amendment — does it change the threshold\?/);
});

test("commit counts come from the committed totals, and failures are said", () => {
  // The producer previews at most ten items, some already present.
  const items = Array.from({ length: 10 }, (_, index) => ({ outcome: index < 4 ? "alreadyStored" : "stored", text: `c${index} Commit ${index}` }));
  assert.equal(
    groupReceipt({ category: "commit", saved: 50, items }),
    "Remembered 50 commits from workspace history: c4 Commit 4; c5 Commit 5; c6 Commit 6; and 47 more",
  );
  assert.equal(
    groupReceipt({ category: "commit", saved: 3, items: items.slice(0, 4) }),
    "Remembered 3 commits from workspace history",
  );
  assert.equal(
    groupReceipt({ category: "commit", saved: 1, failed: 2, items: [{ outcome: "stored", text: "ab12 Fix" }] }),
    "Remembered commit ab12: Fix · from workspace history · 2 could not be saved",
  );
  assert.equal(
    groupReceipt({ category: "commit", saved: 12, failed: 1, items: [{ outcome: "stored", text: "a1 One" }] }),
    "Remembered 12 commits from workspace history: a1 One; and 11 more · 1 could not be saved",
  );
  assert.equal(
    groupReceipt({ category: "commit", saved: 0, failed: 2, items: [] }),
    "No commit from workspace history could be saved · 2 could not be saved",
  );
});

test("an approval arriving or resolving updates the header badge", () => {
  const state = liveState();
  const approval = {
    id: 9,
    method: "item/commandExecution/requestApproval",
    params: { threadId: "thread-a", turnId: "t", itemId: "i", startedAtMs: 1 },
  };
  assert.ok(applyWorkspaceEvent(state, approval).sections.includes("header"));
  assert.ok(
    applyWorkspaceEvent(state, { method: "serverRequest/resolved", params: { threadId: "thread-a", requestId: 9 } })
      .sections.includes("header"),
  );
});

test("a failed first summary marks the session totals as starting late", async () => {
  let fail = true;
  const read = createSummaryReader(async () => {
    if (fail) throw new Error("temporarily unavailable");
    return { counts: { rules: 1 }, latestSequence: 11, since: null };
  }, { threadId: "thread-a" });
  assert.deepEqual(await read(), { error: "temporarily unavailable" });
  fail = false;
  const recovered = await read();
  assert.equal(recovered.sessionStartedLate, true);
  assert.match(memoryStatusLine(recovered), /changes before memory status was available are not counted for this session$/);
  assert.equal((await read()).sessionStartedLate, true);
});

test("concurrent summary reads complete in order and an older one never replaces a newer one", async () => {
  const pending = [];
  const read = createSummaryReader((method, params) => new Promise((resolve) => pending.push({ params, resolve })), {
    threadId: "thread-a",
  });
  const boot = read();
  const background = read();
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(pending.length, 1, "the second read waits for the first");
  pending[0].resolve({ counts: {}, latestSequence: 10, since: null });
  const first = await boot;
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(pending[1].params.sinceSequence, 10, "the second read uses the baseline the first set");
  pending[1].resolve({ counts: { rules: 1 }, latestSequence: 11, since: { saved: 1 } });
  const second = await background;
  assert.equal(newerSummary(second, first), second);
  assert.equal(newerSummary(first, second), second);
  assert.equal(newerSummary(second, { error: "x" }).error, "x");
});

test("a late session start stays qualified across a reload of the tab", async () => {
  const storage = new Map();
  const store = {
    getItem: (key) => storage.get(key) ?? null,
    setItem: (key, value) => storage.set(key, value),
    removeItem: (key) => storage.delete(key),
  };
  let sequence = 10;
  let fail = true;
  const rpc = async (method, params) => {
    if (fail) throw new Error("unavailable");
    return { counts: { rules: 2 }, latestSequence: sequence, since: params.sinceSequence === null ? null : { saved: 1 } };
  };
  const read = createSummaryReader(rpc, { threadId: "thread-a", storage: store });
  await read();
  fail = false;
  sequence = 11;
  assert.equal((await read()).sessionStartedLate, true);
  sequence = 12;
  const reloaded = await createSummaryReader(rpc, { threadId: "thread-a", storage: store })();
  assert.equal(storage.get("stateful-memory-since:thread-a"), "11");
  assert.equal(reloaded.sessionStartedLate, true);
  assert.match(memoryStatusLine(reloaded), /this session: saved 1 · changes before memory status was available are not counted/);
  // A session that started on time is not qualified after a reload.
  const clean = new Map();
  const cleanStore = { getItem: (key) => clean.get(key) ?? null, setItem: (key, value) => clean.set(key, value), removeItem: (key) => clean.delete(key) };
  await createSummaryReader(rpc, { threadId: "thread-b", storage: cleanStore })();
  assert.equal((await createSummaryReader(rpc, { threadId: "thread-b", storage: cleanStore })()).sessionStartedLate, false);
});
