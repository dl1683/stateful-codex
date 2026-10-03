import assert from "node:assert/strict";
import { readdir, readFile } from "node:fs/promises";
import test from "node:test";

import {
  WEB_RPC_METHODS,
  admitApiRequest,
  admitRequest,
  admitRpcMethod,
  checkRpcParams,
  hasSessionCookie,
  isApiPath,
  isJsonObject,
  isRequestId,
  sessionCookie,
} from "../gateway-policy.mjs";

const PORT = 4231;
const TOKEN = "session-token-for-tests";
const own = { host: `127.0.0.1:${PORT}` };
const withCookie = { ...own, cookie: `other=1; stateful_session_${PORT}=${TOKEN}` };

test("pages are served only to the gateway's own host names", () => {
  assert.equal(admitRequest({ headers: own }, PORT), null);
  assert.equal(admitRequest({ headers: { host: `LOCALHOST:${PORT}` } }, PORT), null);
  for (const host of [`attacker.example:${PORT}`, "127.0.0.1", `127.0.0.1:${PORT + 1}`, `[::1]:${PORT}`, undefined]) {
    assert.deepEqual(admitRequest({ headers: { host } }, PORT), {
      status: 421,
      message: "this gateway only answers its own local address",
    });
  }
});

test("a foreign Origin is refused even with the right Host", () => {
  assert.deepEqual(
    admitRequest({ headers: { ...own, origin: "http://attacker.example" } }, PORT),
    { status: 403, message: "cross-origin requests are not allowed" },
  );
  assert.deepEqual(admitRequest({ headers: { ...own, origin: "null" } }, PORT), {
    status: 403,
    message: "cross-origin requests are not allowed",
  });
  assert.equal(
    admitRequest({ headers: { ...own, origin: `http://localhost:${PORT}` } }, PORT),
    null,
  );
});

test("API calls must be same-origin, carry the session cookie, and POSTs the page origin", () => {
  const origin = `http://127.0.0.1:${PORT}`;
  const admit = (method, headers) => admitApiRequest({ method, headers }, PORT, TOKEN);
  assert.equal(admit("POST", { ...withCookie, origin, "sec-fetch-site": "same-origin" }), null);
  assert.equal(admit("GET", withCookie), null);
  assert.deepEqual(admit("POST", withCookie), {
    status: 403,
    message: "requests must come from the Stateful Codex page",
  });
  assert.deepEqual(admit("GET", { ...withCookie, "sec-fetch-site": "cross-site" }), {
    status: 403,
    message: "cross-site requests are not allowed",
  });
  const noSession = {
    status: 403,
    message: "open Stateful Codex from its own address to start a session",
  };
  assert.deepEqual(admit("GET", own), noSession);
  assert.deepEqual(admit("GET", { ...own, cookie: `stateful_session_${PORT}=wrong-token-for-test` }), noSession);
  // Another gateway's cookie on a different port does not authenticate this one.
  assert.deepEqual(admit("GET", { ...own, cookie: `stateful_session_${PORT + 1}=${TOKEN}` }), noSession);
});

test("the session cookie is HttpOnly, same-site and named for its gateway", () => {
  assert.equal(
    sessionCookie(PORT, TOKEN),
    `stateful_session_${PORT}=${TOKEN}; HttpOnly; SameSite=Strict; Path=/`,
  );
  assert.equal(hasSessionCookie({ cookie: sessionCookie(PORT, TOKEN).split(";")[0] }, PORT, TOKEN), true);
});

test("execution methods accept exactly the parameters the page sends", () => {
  const text = [{ type: "text", text: "hi", text_elements: [] }];
  assert.equal(checkRpcParams("turn/start", { threadId: "t", input: text }), null);
  assert.equal(
    checkRpcParams("thread/start", { cwd: "C:/p", runtimeWorkspaceRoots: ["C:/p"], projectId: "p" }),
    null,
  );
  assert.equal(checkRpcParams("thread/resume", { threadId: "t", excludeTurns: true }), null);
  assert.equal(checkRpcParams("project/list", { anything: 1 }), null);
  assert.equal(
    checkRpcParams("thread/start", { cwd: "C:/p", config: { sandbox_mode: "danger-full-access" } }),
    "thread/start does not accept config through the web gateway",
  );
  assert.equal(
    checkRpcParams("turn/start", { threadId: "t", input: text, approvalPolicy: "never" }),
    "turn/start does not accept approvalPolicy through the web gateway",
  );
  assert.equal(
    checkRpcParams("turn/start", { threadId: "t", input: [{ type: "localImage", path: "C:/secret.png" }] }),
    "turn/start input has an unsupported value",
  );
  assert.equal(
    checkRpcParams("thread/resume", { threadId: "t", excludeTurns: "yes" }),
    "thread/resume excludeTurns has an unsupported value",
  );
  assert.equal(checkRpcParams("thread/fork", null), "thread/fork parameters must be an object");
});

test("execution methods require every parameter the page always sends", () => {
  assert.equal(checkRpcParams("thread/start", {}), "thread/start requires cwd");
  assert.equal(
    checkRpcParams("thread/start", { cwd: "C:/outside" }),
    "thread/start requires runtimeWorkspaceRoots",
  );
  assert.equal(
    checkRpcParams("thread/start", { cwd: "C:/p", runtimeWorkspaceRoots: ["C:/p"] }),
    "thread/start requires projectId",
  );
  assert.equal(checkRpcParams("thread/resume", { threadId: "t" }), "thread/resume requires excludeTurns");
  assert.equal(checkRpcParams("thread/fork", { threadId: "t" }), "thread/fork requires cwd");
  assert.equal(checkRpcParams("turn/start", { threadId: "t" }), "turn/start requires input");
  assert.equal(checkRpcParams("turn/start", {}), "turn/start requires threadId");
});

test("every API path needs the session, page assets do not", () => {
  for (const path of ["/events", "/health", "/reply", "/rpc"]) assert.equal(isApiPath(path), true, path);
  for (const path of ["/", "/index.html", "/app.mjs", "/styles.css"]) assert.equal(isApiPath(path), false, path);
});

test("request bodies must be JSON objects and reply ids strings or integers", () => {
  assert.deepEqual(
    [null, [], "x", 3, {}].map(isJsonObject),
    [false, false, false, false, true],
  );
  assert.deepEqual(
    ["1", 1, 1.5, null, true, [], {}].map(isRequestId),
    [true, true, false, false, false, false, false],
  );
});

test("only the methods the web client uses are forwarded", () => {
  for (const method of ["command/exec", "project/delete", "blackboard/upsert", "fs/writeFile", "config/batchWrite"]) {
    assert.equal(admitRpcMethod(method), false, method);
  }
  assert.equal(admitRpcMethod("turn/start"), true);
});

test("the allowlist covers every method the client calls", async () => {
  const publicDir = new URL("../public/", import.meta.url);
  const called = new Set();
  for (const name of await readdir(publicDir)) {
    if (!name.endsWith(".mjs")) continue;
    const source = await readFile(new URL(name, publicDir), "utf8");
    for (const match of source.matchAll(/rpc\(\s*"([A-Za-z]+\/[A-Za-z/]+)"/g)) called.add(match[1]);
  }
  for (const control of ["pause", "resume", "cancel"]) called.add(`statefulRun/${control}`);
  assert.deepEqual([...called].filter((method) => !WEB_RPC_METHODS.has(method)), []);
});

test("memory methods accept exactly the shapes the page sends", () => {
  assert.equal(checkRpcParams("statefulMemory/read", { threadId: "t", cursor: null, limit: 50 }), null);
  assert.equal(
    checkRpcParams("statefulMemory/forget", { threadId: "t", entryId: "e", expectedRevision: 2 }),
    null,
  );
  assert.equal(
    checkRpcParams("statefulMemory/correct", {
      threadId: "t",
      entryId: "e",
      expectedRevision: 2,
      content: "Run only the affected tests.",
    }),
    null,
  );
  assert.equal(
    checkRpcParams("statefulMemory/forget", { threadId: "t", entryId: "e" }),
    "statefulMemory/forget requires expectedRevision",
  );
  assert.equal(
    checkRpcParams("statefulMemory/correct", {
      threadId: "t",
      entryId: "e",
      expectedRevision: 2,
      content: "x".repeat(2001),
    }),
    "statefulMemory/correct content has an unsupported value",
  );
  assert.equal(
    checkRpcParams("statefulMemory/read", { threadId: "t", cursor: null, limit: 500 }),
    "statefulMemory/read limit has an unsupported value",
  );
});
