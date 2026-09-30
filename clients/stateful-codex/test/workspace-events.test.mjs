import assert from "node:assert/strict";
import test from "node:test";

import { applyWorkspaceEvent } from "../public/workspace-events.mjs";

function workspaceState() {
  return {
    threadId: "thread-a",
    projectId: "project-1",
    run: { id: "run-1" },
    pendingRequests: [],
    liveText: "",
    notice: null,
  };
}

const delta = (threadId, text) => ({
  method: "item/agentMessage/delta",
  params: { threadId, turnId: "turn", itemId: "item", delta: text },
});

test("interleaved streams from two threads never mix", () => {
  const state = workspaceState();
  for (const message of [
    delta("thread-a", "asbestos risk "),
    delta("thread-b", "src/click/core.py "),
    delta("thread-a", "is uninsured"),
    delta("thread-b", "final check"),
  ]) {
    applyWorkspaceEvent(state, message);
  }
  assert.equal(state.liveText, "asbestos risk is uninsured");
});

test("another thread's turn start does not clear this thread's live text", () => {
  const state = workspaceState();
  applyWorkspaceEvent(state, delta("thread-a", "partial answer"));
  const foreign = applyWorkspaceEvent(state, {
    method: "turn/started",
    params: { threadId: "thread-b", turn: { id: "t2" } },
  });
  assert.deepEqual(foreign, { render: false, refresh: false });
  assert.equal(state.liveText, "partial answer");
  applyWorkspaceEvent(state, {
    method: "turn/started",
    params: { threadId: "thread-a", turn: { id: "t3" } },
  });
  assert.equal(state.liveText, "");
});

test("only this thread's requests enter the drawer, and resolution removes them", () => {
  const state = workspaceState();
  const own = {
    id: 11,
    method: "item/commandExecution/requestApproval",
    params: { threadId: "thread-a", turnId: "t", itemId: "i", startedAtMs: 1 },
  };
  const foreign = { ...own, id: 12, params: { ...own.params, threadId: "thread-b" } };
  assert.deepEqual(applyWorkspaceEvent(state, own), { render: true, refresh: false });
  assert.deepEqual(applyWorkspaceEvent(state, foreign), { render: false, refresh: false });
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

test("gateway errors reach every workspace", () => {
  const state = workspaceState();
  applyWorkspaceEvent(state, { method: "gateway/error", params: { message: "Reconnecting" } });
  assert.equal(state.notice, "Reconnecting");
});
