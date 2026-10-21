import assert from "node:assert/strict";
import { once } from "node:events";
import { request } from "node:http";
import test from "node:test";

import { EventRouter } from "../event-router.mjs";
import { createGateway } from "../server.mjs";
import { createGatewayFixture } from "./gateway-fixture.mjs";

test("running gateway admits its page and refuses forged HTTP and RPC requests", async (t) => {
  const calls = [];
  const replies = [];
  const router = new EventRouter({ replyToServer: (message) => replies.push(message) });
  const bridge = {
    ready: Promise.resolve(),
    request: async (method, params) => {
      calls.push({ method, params });
      return { data: [] };
    },
    reply: (envelope) => router.reply(envelope),
    subscribe: (response, scope) => router.subscribe({
      write: (message) => response.write(`data: ${JSON.stringify(message)}\n\n`),
    }, scope),
  };
  const server = createGateway(bridge);
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  t.after(() => { server.closeAllConnections(); server.close(); });
  const port = server.address().port;
  const origin = `http://127.0.0.1:${port}`;
  const send = (path, { method = "GET", headers = {}, body } = {}) => new Promise((resolve, reject) => {
    const req = request(`${origin}${path}`, { method, headers }, (response) => {
      const chunks = [];
      response.on("data", (chunk) => chunks.push(chunk));
      response.on("end", () => resolve({
        status: response.statusCode, headers: response.headers, text: Buffer.concat(chunks).toString(),
      }));
    });
    req.on("error", reject);
    req.end(body);
  });
  assert.equal((await send("/", { headers: { host: `evil.invalid:${port}` } })).status, 421);
  assert.equal((await send("/", { headers: { origin: "https://evil.invalid" } })).status, 403);
  const page = await send("/");
  assert.equal(page.status, 200);
  assert.doesNotMatch(page.text, /stateful-session|SESSION_TOKEN/);
  const cookie = page.headers["set-cookie"][0].split(";")[0];
  assert.equal(cookie.split("=")[0], `stateful_session_${port}`);
  assert.match(page.headers["set-cookie"][0], /HttpOnly; SameSite=Strict/);
  assert.equal((await send("/health")).status, 403);
  assert.equal((await send("/health", { headers: { cookie: "stateful_session_" + port + "=forged" } })).status, 403);
  assert.equal((await send("/health", { headers: { cookie } })).status, 200);
  const post = (path, body, extra = {}) => send(path, {
    method: "POST", headers: { cookie, origin, "content-type": "application/json", ...extra },
    body: JSON.stringify(body),
  });
  assert.equal((await post("/rpc", { method: "project/list" }, { origin: "http://evil.invalid" })).status, 403);
  for (const body of [null, [], { method: "turn/start", params: [] }, { method: "project/list", params: null }]) {
    assert.equal((await post("/rpc", body)).status, 400);
  }
  for (const extra of [{ id: 1 }, { jsonrpc: "2.0" }]) {
    assert.equal((await post("/rpc", { method: "project/list", params: {}, ...extra })).status, 400);
  }
  assert.equal((await post("/rpc", {
    method: "project/list", params: { padding: "x".repeat(1024 * 1024) },
  })).status, 413);
  assert.equal((await post("/rpc", { method: "command/exec", params: {} })).status, 403);
  assert.deepEqual(calls, []);
  assert.equal((await post("/rpc", { method: "project/list", params: { limit: 100 } })).status, 200);
  assert.deepEqual(calls, [{ method: "project/list", params: { limit: 100 } }]);
  router.route({ id: 9, method: "item/commandExecution/requestApproval", params: { threadId: "t" } });
  assert.equal((await post("/reply", { threadId: "other", response: { id: 9, result: { decision: "accept" } } })).status, 409);
  assert.equal((await post("/reply", { threadId: "t", response: { id: 9, result: { answers: {} } } })).status, 400);
  assert.deepEqual(replies, []);
  assert.equal((await post("/reply", { threadId: "t", response: { id: 9, result: { decision: "decline" } } })).status, 202);
  assert.deepEqual(replies, [{ id: 9, result: { decision: "decline" } }]);
  assert.equal((await send("/events?token=forged")).status, 403);
});

test("HTTP memory contracts preserve project and revision refusals through the gateway", async (t) => {
  const server = createGateway(createGatewayFixture("C:/disposable/gateway-project"));
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  t.after(() => { server.closeAllConnections(); server.close(); });
  const origin = `http://127.0.0.1:${server.address().port}`;
  const page = await fetch(origin);
  const cookie = page.headers.get("set-cookie").split(";")[0];
  await page.text();
  const call = async (method, params) => {
    const response = await fetch(`${origin}/rpc`, {
      method: "POST", headers: { cookie, origin, "content-type": "application/json" },
      body: JSON.stringify({ method, params }),
    });
    return { status: response.status, body: await response.json() };
  };
  const binding = { threadId: "fixture-thread", expectedProjectId: "fixture-project" };
  const read = { ...binding, cursor: null, limit: 50, backgroundSection: true };
  assert.equal((await call("statefulMemory/read", read)).body.result.projectId, "fixture-project");
  for (const method of ["statefulMemory/forget", "statefulMemory/correct"]) {
    const params = { ...binding, entryId: "fixture-rule", expectedRevision: 1,
      ...(method.endsWith("/correct") ? { content: "New words", backgroundSection: true } : {}) };
    const foreign = await call(method, { ...params, expectedProjectId: "other-project" });
    assert.equal(foreign.body.error.code, -32602);
    assert.match(foreign.body.error.message, /wrong project/);
    const stale = await call(method, { ...params, expectedRevision: 99 });
    assert.equal(stale.body.error.code, -32602);
    assert.match(stale.body.error.message, /stale revision/);
  }
  const addition = { ...binding, kind: "background", content: "Browser user background", reason: null,
    clientActionId: "http-addition", backgroundSection: true };
  const added = await call("statefulMemory/add", addition);
  assert.equal(added.body.result.item.section, "background");
  assert.equal((await call("statefulMemory/add", addition)).body.result.outcome, "alreadyDone");
  assert.equal((await call("statefulMemory/add", { ...addition, content: "é".repeat(1001) })).status, 400);
  assert.equal((await call("statefulMemory/add", { ...addition, expectedProjectId: "foreign" })).body.error.code, -32602);
  const first = await call("statefulMemory/read", read);
  assert.equal(first.body.result.data.find((item) => item.entryId === "fixture-rule").content,
    "Do not publish this disposable project.");
  for (const method of ["statefulMemory/forget", "statefulMemory/correct"]) {
    const params = { ...addition, clientActionId: `retired-${method}` };
    // Simulate an accepted Add whose response never reached the caller.
    const { item } = (await call("statefulMemory/add", params)).body.result;
    const mutation = await call(method, {
      ...binding, entryId: item.entryId, expectedRevision: item.revision,
      ...(method.endsWith("/correct") ? { content: "Replacement words", backgroundSection: true } : {}),
    });
    assert.equal(mutation.status, 200);
    const before = await call("statefulMemory/read", read);
    const replay = await call("statefulMemory/add", params);
    assert.equal(replay.body.error.code, -32602);
    assert.match(replay.body.error.message, /entry is retired; nothing was restored/);
    const after = await call("statefulMemory/read", read);
    assert.deepEqual(after, before);
    assert.equal(after.body.result.data.some((entry) => entry.entryId === item.entryId), false);
    if (method.endsWith("/correct")) {
      assert.deepEqual(after.body.result.data[0], mutation.body.result.item);
    }
  }
});
