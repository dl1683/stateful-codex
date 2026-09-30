import assert from "node:assert/strict";
import test from "node:test";

import { EventRouter } from "../event-router.mjs";

function setup() {
  const toServer = [];
  const router = new EventRouter({ replyToServer: (message) => toServer.push(message) });
  const sink = () => {
    const received = [];
    return { received, write: (message) => received.push(message) };
  };
  return { router, toServer, sink };
}

const delta = (threadId, text) => ({
  method: "item/agentMessage/delta",
  params: { threadId, turnId: "t", itemId: "i", delta: text },
});
const approval = (id, threadId) => ({
  id,
  method: "item/commandExecution/requestApproval",
  params: { threadId, turnId: "t", itemId: "i", startedAtMs: 1 },
});

test("each workspace receives only its own thread's stream and requests", () => {
  const { router, sink } = setup();
  const a = sink();
  const b = sink();
  const setupPage = sink();
  router.subscribe(a, { threadId: "thread-a", projectId: "project-1" });
  router.subscribe(b, { threadId: "thread-b", projectId: "project-2" });
  router.subscribe(setupPage);
  for (const message of [delta("thread-a", "one"), delta("thread-b", "two"), approval(1, "thread-b")]) {
    router.route(message);
  }
  assert.deepEqual(a.received, [delta("thread-a", "one")]);
  assert.deepEqual(b.received, [delta("thread-b", "two"), approval(1, "thread-b")]);
  assert.deepEqual(setupPage.received, []);
});

test("gateway errors reach every subscription", () => {
  const { router, sink } = setup();
  const a = sink();
  const setupPage = sink();
  router.subscribe(a, { threadId: "thread-a", projectId: "project-1" });
  router.subscribe(setupPage);
  const error = { method: "gateway/error", params: { message: "app-server exited" } };
  router.route(error);
  assert.deepEqual(a.received, [error]);
  assert.deepEqual(setupPage.received, [error]);
});

test("a sibling thread's same-project notification reaches the workspace for invalidation", () => {
  const { router, sink } = setup();
  const a = sink();
  router.subscribe(a, { threadId: "thread-a", projectId: "project-1" });
  const sibling = {
    method: "statefulAttribution/completed",
    params: { projectId: "project-1", threadId: "thread-b", turnId: "t", status: "completed" },
  };
  const other = { ...sibling, params: { ...sibling.params, projectId: "project-2" } };
  router.route(sibling);
  router.route(other);
  assert.deepEqual(a.received, [sibling]);
});

test("replies are forwarded only from the owning thread, once", () => {
  const { router, toServer } = setup();
  router.route(approval(7, "thread-a"));
  assert.deepEqual(router.reply({ threadId: "thread-b", response: { id: 7, result: {} } }), {
    accepted: false,
    reason: "request belongs to another thread",
  });
  assert.deepEqual(toServer, []);
  assert.deepEqual(router.reply({ threadId: "thread-a", response: { id: 7, result: { decision: "accept" } } }), {
    accepted: true,
  });
  assert.deepEqual(toServer, [{ id: 7, result: { decision: "accept" } }]);
  assert.equal(
    router.reply({ threadId: "thread-a", response: { id: 7, result: {} } }).accepted,
    false,
  );
});

test("request ids keep their JSON type", () => {
  const { router, toServer } = setup();
  router.route(approval(1, "thread-a"));
  assert.equal(router.reply({ threadId: "thread-a", response: { id: "1", result: {} } }).accepted, false);
  assert.equal(router.reply({ threadId: "thread-a", response: { id: 1, result: {} } }).accepted, true);
  assert.deepEqual(toServer, [{ id: 1, result: {} }]);
});

test("resolved requests can no longer be answered and are not replayed", () => {
  const { router, sink } = setup();
  router.route(approval(3, "thread-a"));
  router.route({ method: "serverRequest/resolved", params: { threadId: "thread-a", requestId: 3 } });
  assert.equal(router.reply({ threadId: "thread-a", response: { id: 3, result: {} } }).accepted, false);
  const reconnect = sink();
  router.subscribe(reconnect, { threadId: "thread-a", projectId: "project-1" });
  assert.deepEqual(reconnect.received, []);
});

test("a reconnecting workspace gets its own outstanding requests replayed", () => {
  const { router, sink } = setup();
  router.route(approval(4, "thread-a"));
  router.route(approval(5, "thread-b"));
  const reconnect = sink();
  router.subscribe(reconnect, { threadId: "thread-a", projectId: "project-1" });
  assert.deepEqual(reconnect.received, [approval(4, "thread-a")]);
});

test("global and unscoped server requests are answered with an error, not shown anywhere", () => {
  const { router, toServer, sink } = setup();
  const a = sink();
  router.subscribe(a, { threadId: "thread-a", projectId: "project-1" });
  router.route({ id: 9, method: "account/chatgptAuthTokens/refresh", params: {} });
  assert.deepEqual(a.received, []);
  assert.equal(toServer.length, 1);
  assert.equal(toServer[0].id, 9);
  assert.equal(toServer[0].error.code, -32601);
});

test("unsubscribing one workspace leaves the others connected", () => {
  const { router, sink } = setup();
  const a = sink();
  const b = sink();
  const unsubscribeA = router.subscribe(a, { threadId: "thread-a", projectId: "project-1" });
  router.subscribe(b, { threadId: "thread-a", projectId: "project-1" });
  unsubscribeA();
  router.route(delta("thread-a", "still here"));
  assert.deepEqual(a.received, []);
  assert.deepEqual(b.received, [delta("thread-a", "still here")]);
});
