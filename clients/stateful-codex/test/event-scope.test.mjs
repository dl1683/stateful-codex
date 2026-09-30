import assert from "node:assert/strict";
import test from "node:test";

import { belongsToWorkspace, eventScope } from "../public/event-scope.mjs";

const owner = { threadId: "thread-a", projectId: "project-1", runId: "run-1" };

test("classifies app-server messages by their owning scope", () => {
  assert.deepEqual(
    eventScope({
      method: "item/agentMessage/delta",
      params: { threadId: "thread-a", turnId: "t", itemId: "i", delta: "x" },
    }),
    { kind: "thread", threadId: "thread-a" },
  );
  assert.deepEqual(
    eventScope({ method: "thread/started", params: { thread: { id: "thread-b" } } }),
    { kind: "thread", threadId: "thread-b" },
  );
  assert.deepEqual(
    eventScope({
      id: 7,
      method: "item/commandExecution/requestApproval",
      params: { threadId: "thread-a", turnId: "t", itemId: "i", startedAtMs: 1 },
    }),
    { kind: "threadRequest", threadId: "thread-a" },
  );
  assert.deepEqual(
    eventScope({
      method: "statefulRun/updated",
      params: { projectId: "project-1", runId: "run-9", revision: 1, cursor: "c" },
    }),
    { kind: "run", projectId: "project-1", runId: "run-9" },
  );
  assert.deepEqual(
    eventScope({ method: "project/changed", params: { projectId: "project-1" } }),
    { kind: "project", projectId: "project-1" },
  );
  assert.deepEqual(eventScope({ method: "gateway/error", params: {} }), {
    kind: "gateway",
  });
  assert.deepEqual(
    eventScope({ id: 3, method: "account/chatgptAuthTokens/refresh", params: {} }),
    { kind: "globalRequest" },
  );
  assert.deepEqual(eventScope({ method: "mcpServer/startup", params: {} }), {
    kind: "unknown",
  });
});

test("a workspace owns only its thread, project and current run", () => {
  const scope = (message) => belongsToWorkspace(eventScope(message), owner);
  assert.equal(
    scope({ method: "item/agentMessage/delta", params: { threadId: "thread-a" } }),
    true,
  );
  assert.equal(
    scope({ method: "item/agentMessage/delta", params: { threadId: "thread-b" } }),
    false,
  );
  assert.equal(
    scope({ id: 1, method: "item/tool/requestUserInput", params: { threadId: "thread-b" } }),
    false,
  );
  assert.equal(scope({ method: "blackboard/updated", params: { projectId: "project-2" } }), false);
  assert.equal(
    scope({ method: "statefulRun/updated", params: { projectId: "project-1", runId: "run-2" } }),
    false,
  );
  assert.equal(
    scope({ method: "statefulRun/updated", params: { projectId: "project-1", runId: "run-1" } }),
    true,
  );
  assert.equal(scope({ id: 4, method: "attestation/generate", params: {} }), false);
  assert.equal(scope({ method: "gateway/connected", params: {} }), true);
});

test("before its run is known a workspace accepts its project's run updates", () => {
  assert.equal(
    belongsToWorkspace(eventScope({
      method: "statefulRun/updated",
      params: { projectId: "project-1", runId: "run-7" },
    }), { ...owner, runId: null }),
    true,
  );
});
