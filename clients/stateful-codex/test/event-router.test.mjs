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

const snapshot = (threadId, requests) => ({
  method: "gateway/pendingRequests",
  params: { threadId, requests },
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
  assert.deepEqual(a.received, [snapshot("thread-a", []), delta("thread-a", "one")]);
  assert.deepEqual(b.received, [snapshot("thread-b", []), delta("thread-b", "two"), approval(1, "thread-b")]);
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
  assert.deepEqual(a.received, [snapshot("thread-a", []), error]);
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
  assert.deepEqual(a.received, [snapshot("thread-a", []), sibling]);
});

test("replies are forwarded only from the owning thread, once", () => {
  const { router, toServer } = setup();
  router.route(approval(7, "thread-a"));
  assert.deepEqual(router.reply({ threadId: "thread-b", response: { id: 7, result: {} } }), {
    accepted: false,
    code: "foreignThread",
    reason: "request belongs to another thread",
  });
  assert.deepEqual(toServer, []);
  assert.deepEqual(
    router.reply({ threadId: "thread-a", response: { id: 7, result: { decision: "accept" } } }),
    { accepted: true },
  );
  assert.deepEqual(toServer, [{ id: 7, result: { decision: "accept" } }]);
  assert.deepEqual(router.reply({ threadId: "thread-a", response: { id: 7, result: {} } }), {
    accepted: false,
    code: "resolved",
    reason: "request is no longer open",
  });
});

test("request ids keep their JSON type", () => {
  const { router, toServer } = setup();
  router.route(approval(1, "thread-a"));
  assert.deepEqual(router.reply({
    threadId: "thread-a", response: { id: "1", result: { decision: "accept" } },
  }), { accepted: false, code: "resolved", reason: "request is no longer open" });
  assert.deepEqual(toServer, []);
  assert.equal(router.reply({ threadId: "thread-a", response: { id: 1, result: { decision: "accept" } } }).accepted, true);
  assert.deepEqual(toServer, [{ id: 1, result: { decision: "accept" } }]);
});

test("numeric and string request ids coexist and resolve independently", () => {
  const { router, toServer } = setup();
  router.route(approval(1, "thread-a"));
  router.route(approval("1", "thread-a"));
  assert.deepEqual(router.pendingSnapshot("thread-a"), snapshot("thread-a", [
    approval(1, "thread-a"), approval("1", "thread-a"),
  ]));
  assert.deepEqual(router.reply({
    threadId: "thread-a", response: { id: 1, result: { decision: "decline" } },
  }), { accepted: true });
  assert.deepEqual(router.pendingSnapshot("thread-a"), snapshot("thread-a", [approval("1", "thread-a")]));
  assert.deepEqual(router.reply({
    threadId: "thread-a", response: { id: "1", result: { decision: "accept" } },
  }), { accepted: true });
  assert.deepEqual(toServer, [
    { id: 1, result: { decision: "decline" } },
    { id: "1", result: { decision: "accept" } },
  ]);
  assert.deepEqual(router.pendingSnapshot("thread-a"), snapshot("thread-a", []));
});

test("a workspace subscription starts with an authoritative snapshot of its open requests", () => {
  const { router, sink } = setup();
  router.route(approval(4, "thread-a"));
  router.route(approval(5, "thread-b"));
  const workspace = sink();
  const setupPage = sink();
  router.subscribe(workspace, { threadId: "thread-a", projectId: "project-1" });
  router.subscribe(setupPage);
  assert.deepEqual(workspace.received, [snapshot("thread-a", [approval(4, "thread-a")])]);
  assert.deepEqual(setupPage.received, []);
});

test("a tab that was away while another tab answered reconnects to an empty drawer", () => {
  const { router, sink } = setup();
  const tabA = sink();
  const tabB = sink();
  const unsubscribeA = router.subscribe(tabA, { threadId: "thread-a", projectId: "project-1" });
  router.subscribe(tabB, { threadId: "thread-a", projectId: "project-1" });
  router.route(approval(8, "thread-a"));
  unsubscribeA();
  assert.equal(router.reply({ threadId: "thread-a", response: { id: 8, result: { decision: "decline" } } }).accepted, true);
  const tabAAgain = sink();
  router.subscribe(tabAAgain, { threadId: "thread-a", projectId: "project-1" });
  assert.deepEqual(tabAAgain.received, [snapshot("thread-a", [])]);
});

test("resolution and turn completion retire open requests", () => {
  const { router, sink } = setup();
  router.route(approval(3, "thread-a"));
  router.route({ method: "serverRequest/resolved", params: { threadId: "thread-a", requestId: 3 } });
  const inTurn = approval(6, "thread-a");
  router.route({ ...inTurn, params: { ...inTurn.params, turnId: "turn-1" } });
  router.route({ method: "turn/completed", params: { threadId: "thread-a", turn: { id: "turn-1" } } });
  const reconnect = sink();
  router.subscribe(reconnect, { threadId: "thread-a", projectId: "project-1" });
  assert.deepEqual(reconnect.received, [snapshot("thread-a", [])]);
  assert.equal(router.reply({ threadId: "thread-a", response: { id: 6, result: {} } }).code, "resolved");
});

test("losing the app-server clears every open request", () => {
  const { router } = setup();
  router.route(approval(2, "thread-a"));
  router.clear();
  assert.equal(router.reply({ threadId: "thread-a", response: { id: 2, result: {} } }).code, "resolved");
});

test("requests the page cannot answer are declined at once and never retained", () => {
  const { router, toServer, sink } = setup();
  const workspace = sink();
  router.subscribe(workspace, { threadId: "thread-a", projectId: "project-1" });
  for (let id = 20; id < 23; id += 1) {
    router.route({ id, method: "currentTime/read", params: { threadId: "thread-a" } });
  }
  router.route({ id: 30, method: "account/chatgptAuthTokens/refresh", params: {} });
  assert.deepEqual(
    toServer.map((message) => [message.id, message.error.code]),
    [
      [20, -32601],
      [21, -32601],
      [22, -32601],
      [30, -32601],
    ],
  );
  assert.deepEqual(workspace.received, [snapshot("thread-a", [])]);
  assert.deepEqual(router.pendingSnapshot("thread-a"), snapshot("thread-a", []));
});

test("unsubscribing one workspace leaves the others connected", () => {
  const { router, sink } = setup();
  const a = sink();
  const b = sink();
  const unsubscribeA = router.subscribe(a, { threadId: "thread-a", projectId: "project-1" });
  router.subscribe(b, { threadId: "thread-a", projectId: "project-1" });
  unsubscribeA();
  router.route(delta("thread-a", "still here"));
  assert.deepEqual(a.received, [snapshot("thread-a", [])]);
  assert.deepEqual(b.received, [snapshot("thread-a", []), delta("thread-a", "still here")]);
});

test("a mismatched reply cannot retire or change the stored request", () => {
  const { router, toServer } = setup();
  router.route(approval(42, "thread-a"));
  for (const response of [
    { id: 42, error: { code: -1, message: "forged" } },
    { id: 42, result: { decision: "acceptForSession" } },
    { id: 42, result: { decision: "accept", extra: true } },
    { id: 42, result: { answers: {} } },
    { id: 42, result: { decision: "accept" }, method: "command/exec" },
  ]) {
    assert.equal(router.reply({ threadId: "thread-a", response }).code, "invalid");
  }
  assert.deepEqual(toServer, []);
  assert.deepEqual(router.pendingSnapshot("thread-a"), snapshot("thread-a", [approval(42, "thread-a")]));
  assert.deepEqual(router.reply({
    threadId: "thread-a", response: { id: 42, result: { decision: "accept" } },
  }), { accepted: true });
});

test("user input replies answer exactly the stored question ids", () => {
  const { router, toServer } = setup();
  router.route({
    id: "question", method: "item/tool/requestUserInput",
    params: { threadId: "thread-a", questions: [{ id: "choice" }] },
  });
  const answer = (answers) => router.reply({
    threadId: "thread-a", response: { id: "question", result: { answers } },
  });
  for (const answers of [{}, { other: { answers: ["x"] } }, { choice: { answers: ["a", "b"] } }]) {
    assert.equal(answer(answers).code, "invalid");
  }
  assert.deepEqual(answer({ choice: { answers: ["a"] } }), { accepted: true });
  assert.deepEqual(toServer, [{ id: "question", result: { answers: { choice: { answers: ["a"] } } } }]);
});
