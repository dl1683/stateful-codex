import assert from "node:assert/strict";
import { once } from "node:events";
import { request } from "node:http";
import test from "node:test";

import { EventRouter } from "../event-router.mjs";
import { createGateway } from "../server.mjs";

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
