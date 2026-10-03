// Request admission for the local web gateway. The gateway holds the user's Codex session, so it
// only answers requests addressed to its own loopback origin (a foreign Host is how DNS
// rebinding reaches a local server), authenticates API calls with an HttpOnly cookie that page
// scripts and URLs never carry, and forwards only the app-server methods and parameter shapes the
// web client uses. Everything else, including command/exec, file writes, project deletion and
// per-thread sandbox or approval overrides, stays unreachable from the browser.

import { timingSafeEqual } from "node:crypto";

// Every app-server method the web client calls. Keep in sync with public/*.mjs.
export const WEB_RPC_METHODS = new Set([
  "account/read",
  "blackboard/confirm",
  "blackboard/query",
  "contextMap/query",
  "contextMap/refresh",
  "evidence/read",
  "fs/getMetadata",
  "obligation/list",
  "project/create",
  "project/list",
  "project/read",
  "projectIntelligence/status",
  "projectIntelligence/tree",
  "statefulMeasurement/list",
  "statefulMemory/add",
  "statefulMemory/correct",
  "statefulMemory/forget",
  "statefulMemory/read",
  "statefulMemory/recap",
  "statefulMemory/summary",
  "statefulMeasurement/summary",
  "statefulRun/cancel",
  "statefulRun/pause",
  "statefulRun/read",
  "statefulRun/resume",
  "statefulRun/setMode",
  "statefulRun/start",
  "steering/list",
  "steering/submit",
  "thread/fork",
  "thread/items/list",
  "thread/list",
  "thread/read",
  "thread/resume",
  "thread/start",
  "thread/turns/list",
  "turn/start",
]);

// The Host values a browser sends when the user opens the gateway's own address.
export function allowedHosts(port) {
  return new Set([`127.0.0.1:${port}`, `localhost:${port}`]);
}

export function allowedOrigins(port) {
  return new Set([...allowedHosts(port)].map((host) => `http://${host}`));
}

// Returns null when the request may proceed, otherwise { status, message }.
// Every request, including the pages that set the session cookie, must name the gateway's own
// Host, and a request that carries an Origin must come from the gateway's own origin.
export function admitRequest({ headers }, port) {
  const host = String(headers.host ?? "").toLowerCase();
  if (!allowedHosts(port).has(host)) {
    return { status: 421, message: "this gateway only answers its own local address" };
  }
  const origin = headers.origin;
  if (origin !== undefined && !allowedOrigins(port).has(String(origin).toLowerCase())) {
    return { status: 403, message: "cross-origin requests are not allowed" };
  }
  return null;
}

// API requests (/rpc, /reply, /events) additionally must not be cross-site, must carry the
// session cookie, and a state-changing request must carry the page's Origin: browsers always send
// it on POST, so its absence means the caller is not the web client.
export function admitApiRequest(request, port, sessionToken) {
  const refused = admitRequest(request, port);
  if (refused) return refused;
  const site = request.headers["sec-fetch-site"];
  if (site !== undefined && site !== "same-origin") {
    return { status: 403, message: "cross-site requests are not allowed" };
  }
  if (request.method !== "GET" && request.headers.origin === undefined) {
    return { status: 403, message: "requests must come from the Stateful Codex page" };
  }
  if (!hasSessionCookie(request.headers, port, sessionToken)) {
    return { status: 403, message: "open Stateful Codex from its own address to start a session" };
  }
  return null;
}

// Cookies are shared across ports of one host, so each gateway uses its own cookie name.
export function sessionCookieName(port) {
  return `stateful_session_${port}`;
}

export function sessionCookie(port, sessionToken) {
  return `${sessionCookieName(port)}=${sessionToken}; HttpOnly; SameSite=Strict; Path=/`;
}

export function hasSessionCookie(headers, port, sessionToken) {
  const name = sessionCookieName(port);
  const presented = String(headers.cookie ?? "")
    .split(";")
    .map((part) => part.trim())
    .find((part) => part.startsWith(`${name}=`))
    ?.slice(name.length + 1);
  if (presented === undefined) return false;
  const expected = Buffer.from(sessionToken);
  const actual = Buffer.from(presented);
  return actual.length === expected.length && timingSafeEqual(actual, expected);
}

// Paths that need the session cookie: everything except the static page assets.
const API_PATHS = new Set(["/events", "/health", "/reply", "/rpc"]);

export function isApiPath(pathname) {
  return API_PATHS.has(pathname);
}

// A JSON-RPC id is a string or an integer.
export function isRequestId(id) {
  return typeof id === "string" || Number.isInteger(id);
}

// A request body that is a JSON object (not null, an array or a scalar).
export function isJsonObject(value) {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

export function admitRpcMethod(method) {
  return WEB_RPC_METHODS.has(method);
}

const isString = (value) => typeof value === "string" && value.length > 0;
const isStringList = (value) => Array.isArray(value) && value.every(isString);
const isBoolean = (value) => typeof value === "boolean";
const isRevision = (value) => Number.isInteger(value) && value > 0;
// The only input the page sends: one plain text item.
const isTextInput = (value) =>
  Array.isArray(value) &&
  value.length === 1 &&
  exactKeys(value[0], {
    type: (type) => type === "text",
    text: (text) => typeof text === "string",
    text_elements: (elements) => Array.isArray(elements) && elements.length === 0,
  });

// Methods that can start or steer execution accept exactly the parameters the page sends (all of
// them required), so a caller cannot pass config, sandbox, approval or model overrides through the
// gateway, or start a thread without its project.
const EXECUTION_PARAMS = {
  "thread/start": { cwd: isString, runtimeWorkspaceRoots: isStringList, projectId: isString },
  "thread/resume": { threadId: isString, excludeTurns: isBoolean },
  "thread/fork": {
    threadId: isString,
    cwd: isString,
    runtimeWorkspaceRoots: isStringList,
    excludeTurns: isBoolean,
  },
  "turn/start": { threadId: isString, input: isTextInput },
  // Memory changes act with the user's authority, so their shapes are exact as well.
  "statefulMemory/read": {
    threadId: isString,
    cursor: (cursor) => cursor === null || isString(cursor),
    limit: (limit) => Number.isInteger(limit) && limit > 0 && limit <= 100,
    backgroundSection: isBoolean,
  },
  "statefulMemory/forget": { threadId: isString, entryId: isString, expectedRevision: isRevision },
  "statefulMemory/correct": {
    threadId: isString,
    entryId: isString,
    expectedRevision: isRevision,
    content: (content) => isString(content) && content.length <= 2000,
    backgroundSection: isBoolean,
  },
  "statefulMemory/add": {
    threadId: isString,
    kind: (kind) => ["rule", "background", "decision", "note"].includes(kind),
    content: (content) => isString(content) && content.trim().length > 0 && content.length <= 2000,
    reason: (reason) => reason === null || (isString(reason) && reason.length <= 1000),
    clientActionId: (id) => isString(id) && id.length > 0 && id.length <= 128,
    backgroundSection: isBoolean,
  },
};

// Returns null when the parameters are acceptable, otherwise a message naming the problem.
export function checkRpcParams(method, params) {
  const shape = EXECUTION_PARAMS[method];
  if (!shape) return null;
  if (!params || typeof params !== "object" || Array.isArray(params)) {
    return `${method} parameters must be an object`;
  }
  for (const [key, value] of Object.entries(params)) {
    if (!Object.hasOwn(shape, key)) return `${method} does not accept ${key} through the web gateway`;
    if (!shape[key](value)) return `${method} ${key} has an unsupported value`;
  }
  const missing = Object.keys(shape).find((key) => !Object.hasOwn(params, key));
  return missing ? `${method} requires ${missing}` : null;
}

// Every key present, none extra, each value accepted by its check.
function exactKeys(value, checks) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const keys = Object.keys(value);
  return (
    keys.length === Object.keys(checks).length &&
    keys.every((key) => Object.hasOwn(checks, key) && checks[key](value[key]))
  );
}

const APPROVAL_REQUESTS = new Set([
  "item/commandExecution/requestApproval",
  "item/fileChange/requestApproval",
]);

// The result a page may send for one stored server request: an approval decision, or one text
// answer per question the request asked. Returns the result to forward, or null if it is not
// exactly that shape.
export function acceptedReplyResult(request, result) {
  if (APPROVAL_REQUESTS.has(request.method)) {
    const decided = exactKeys(result, {
      decision: (decision) => decision === "accept" || decision === "decline",
    });
    return decided ? { decision: result.decision } : null;
  }
  if (request.method === "item/tool/requestUserInput") {
    // The page answers each distinct question id once.
    const questions = [...new Set((request.params?.questions ?? []).map((question) => question.id))];
    if (!exactKeys(result, { answers: isJsonObject })) return null;
    const answered = Object.keys(result.answers);
    const valid =
      answered.length === questions.length &&
      questions.every(
        (id) =>
          Object.hasOwn(result.answers, id) &&
          exactKeys(result.answers[id], {
            answers: (list) => Array.isArray(list) && list.length === 1 && typeof list[0] === "string",
          }),
      );
    if (!valid) return null;
    const answers = questions.map((id) => [id, { answers: [result.answers[id].answers[0]] }]);
    return { answers: Object.fromEntries(answers) };
  }
  return null;
}
