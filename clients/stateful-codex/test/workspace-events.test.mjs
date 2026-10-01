import assert from "node:assert/strict";
import test from "node:test";

import { applyWorkspaceEvent } from "../public/workspace-events.mjs";

function workspaceState() {
  return {
    threadId: "thread-a",
    projectId: "project-1",
    run: { id: "run-1" },
    pendingRequests: [],
    notice: null,
  };
}

const delta = (threadId, text) => ({
  method: "item/agentMessage/delta",
  params: { threadId, turnId: "turn", itemId: "item", delta: text },
});

test("interleaved streams from two threads never mix", () => {
  const state = workspaceState();
  const streamed = [
    delta("thread-a", "asbestos risk "),
    delta("thread-b", "src/click/core.py "),
    delta("thread-a", "is uninsured"),
    delta("thread-b", "final check"),
  ]
    .map((message) => applyWorkspaceEvent(state, message).delta)
    .filter(Boolean);
  assert.deepEqual(streamed, [
    { turnId: "turn", itemId: "item", delta: "asbestos risk " },
    { turnId: "turn", itemId: "item", delta: "is uninsured" },
  ]);
});

test("only this thread's turn start resets the live text", () => {
  const state = workspaceState();
  const foreign = applyWorkspaceEvent(state, {
    method: "turn/started",
    params: { threadId: "thread-b", turn: { id: "t2" } },
  });
  assert.deepEqual(foreign, { sections: [], refresh: false });
  assert.deepEqual(
    applyWorkspaceEvent(state, {
      method: "turn/started",
      params: { threadId: "thread-a", turn: { id: "t3" } },
    }),
    { sections: [], refresh: true, turnStarted: "t3" },
  );
});

test("only this thread's requests enter the drawer, and resolution removes them", () => {
  const state = workspaceState();
  const own = {
    id: 11,
    method: "item/commandExecution/requestApproval",
    params: { threadId: "thread-a", turnId: "t", itemId: "i", startedAtMs: 1 },
  };
  const foreign = { ...own, id: 12, params: { ...own.params, threadId: "thread-b" } };
  assert.deepEqual(applyWorkspaceEvent(state, own), { sections: ["requests"], refresh: false });
  assert.deepEqual(applyWorkspaceEvent(state, foreign), { sections: [], refresh: false });
  assert.deepEqual(state.pendingRequests.map((item) => item.id), [11]);
  applyWorkspaceEvent(state, {
    method: "serverRequest/resolved",
    params: { threadId: "thread-b", requestId: 11 },
  });
  assert.deepEqual(state.pendingRequests.map((item) => item.id), [11]);
  applyWorkspaceEvent(state, {
    method: "serverRequest/resolved",
    params: { threadId: "thread-a", requestId: 11 },
  });
  assert.deepEqual(state.pendingRequests, []);
});

test("global and unscoped requests are not shown in a workspace", () => {
  const state = workspaceState();
  applyWorkspaceEvent(state, { id: 5, method: "account/chatgptAuthTokens/refresh", params: {} });
  applyWorkspaceEvent(state, { id: 6, method: "mystery/request", params: {} });
  assert.deepEqual(state.pendingRequests, []);
});

test("only this workspace's run and project changes trigger a refresh", () => {
  const state = workspaceState();
  const run = (runId) => ({
    method: "statefulRun/updated",
    params: { projectId: "project-1", runId, revision: 2, cursor: "c" },
  });
  assert.equal(applyWorkspaceEvent(state, run("run-1")).refresh, true);
  assert.equal(applyWorkspaceEvent(state, run("run-2")).refresh, false);
  assert.equal(
    applyWorkspaceEvent(state, {
      method: "blackboard/updated",
      params: { projectId: "project-1", entityKind: "entry", entityId: "e", revision: 1, cursor: "c" },
    }).refresh,
    true,
  );
  assert.equal(
    applyWorkspaceEvent(state, {
      method: "blackboard/updated",
      params: { projectId: "project-2", entityKind: "entry", entityId: "e", revision: 1, cursor: "c" },
    }).refresh,
    false,
  );
});

test("a sibling thread's attribution refreshes shared project data without rendering its content", () => {
  const state = workspaceState();
  const attribution = (projectId, threadId) => ({
    method: "statefulAttribution/completed",
    params: { projectId, threadId, turnId: "t", status: "completed", durationMs: 1, counters: {} },
  });
  assert.deepEqual(applyWorkspaceEvent(state, attribution("project-1", "thread-b")), {
    sections: [],
    refresh: true,
  });
  assert.deepEqual(applyWorkspaceEvent(state, attribution("project-2", "thread-b")), {
    sections: [],
    refresh: false,
  });
  assert.deepEqual(state.pendingRequests, []);
});

test("gateway errors reach every workspace", () => {
  const state = workspaceState();
  applyWorkspaceEvent(state, { method: "gateway/error", params: { message: "Reconnecting" } });
  assert.equal(state.notice, "Reconnecting");
});

test("a pending-request snapshot replaces this thread's drawer and ignores other threads", () => {
  const state = workspaceState();
  state.pendingRequests = [
    { id: 1, method: "item/fileChange/requestApproval", params: { threadId: "thread-a" } },
  ];
  const kept = { id: 2, method: "item/fileChange/requestApproval", params: { threadId: "thread-a" } };
  assert.deepEqual(
    applyWorkspaceEvent(state, {
      method: "gateway/pendingRequests",
      params: { threadId: "thread-b", requests: [] },
    }),
    { sections: [], refresh: false },
  );
  assert.equal(state.pendingRequests.length, 1);
  applyWorkspaceEvent(state, {
    method: "gateway/pendingRequests",
    params: { threadId: "thread-a", requests: [kept] },
  });
  assert.deepEqual(state.pendingRequests, [kept]);
});

test("only this thread's finished agent message is passed on for formatting", () => {
  const state = workspaceState();
  const completed = (threadId) => ({
    method: "item/completed",
    params: { threadId, turnId: "turn", item: { type: "agentMessage", id: "item", text: "**done**" } },
  });

  assert.equal(applyWorkspaceEvent(state, completed("thread-b")).completedMessage, undefined);
  assert.deepEqual(applyWorkspaceEvent(state, completed("thread-a")).completedMessage, {
    turnId: "turn",
    itemId: "item",
    text: "**done**",
  });
});
