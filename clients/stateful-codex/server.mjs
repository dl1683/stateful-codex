import { randomBytes } from "node:crypto";
import { existsSync } from "node:fs";
import { readFile, stat } from "node:fs/promises";
import { createServer } from "node:http";
import { dirname, extname, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";

const here = dirname(fileURLToPath(import.meta.url));
const publicRoot = resolve(here, "public");
const host = "127.0.0.1";
const port = Number.parseInt(process.env.STATEFUL_CODEX_PORT ?? "4173", 10);
const sessionToken = randomBytes(24).toString("base64url");
const mimeTypes = new Map([
  [".css", "text/css; charset=utf-8"],
  [".html", "text/html; charset=utf-8"],
  [".js", "text/javascript; charset=utf-8"],
  [".mjs", "text/javascript; charset=utf-8"],
  [".svg", "image/svg+xml"],
]);

class AppServerBridge {
  constructor() {
    this.nextId = 1;
    this.pending = new Map();
    this.listeners = new Set();
    this.ready = this.start();
  }

  async start() {
    const executable = resolveCodexExecutable();
    const environment = { ...process.env };
    delete environment.OPENAI_API_KEY;
    delete environment.CODEX_API_KEY;
    this.child = spawn(
      executable,
      [
        "-c",
        'forced_login_method="chatgpt"',
        "app-server",
        "--listen",
        "stdio://",
      ],
      { env: environment, stdio: ["pipe", "pipe", "inherit"] },
    );
    this.child.once("error", (error) => this.fail(error));
    this.child.once("exit", (code, signal) => {
      this.fail(
        new Error(
          `Codex app-server exited (${code ?? "no code"}, ${signal ?? "no signal"})`,
        ),
      );
    });
    createInterface({ input: this.child.stdout }).on("line", (line) => {
      this.receive(line);
    });
    await this.requestWithoutReady("initialize", {
      clientInfo: {
        name: "stateful-codex-web",
        title: "Stateful Codex",
        version: "0.1.0",
      },
      capabilities: { experimentalApi: true },
    });
    this.send({ method: "initialized" });
  }

  request(method, params = {}) {
    return this.ready.then(() => this.requestWithoutReady(method, params));
  }

  requestWithoutReady(method, params) {
    const id = this.nextId++;
    const promise = new Promise((resolveRequest, rejectRequest) => {
      this.pending.set(id, { resolve: resolveRequest, reject: rejectRequest });
    });
    this.send({ id, method, params });
    return promise;
  }

  reply(message) {
    this.send(message);
  }

  subscribe(response) {
    this.listeners.add(response);
    return () => this.listeners.delete(response);
  }

  send(message) {
    if (!this.child?.stdin.writable) {
      throw new Error("Codex app-server input is unavailable");
    }
    this.child.stdin.write(`${JSON.stringify(message)}\n`);
  }

  receive(line) {
    let message;
    try {
      message = JSON.parse(line);
    } catch {
      return;
    }
    if (Object.hasOwn(message, "id") && !Object.hasOwn(message, "method")) {
      const request = this.pending.get(message.id);
      if (!request) return;
      this.pending.delete(message.id);
      if (message.error) {
        request.reject(new RpcError(message.error));
      } else {
        request.resolve(message.result);
      }
      return;
    }
    this.broadcast(message);
  }

  broadcast(message) {
    const data = `data: ${JSON.stringify(message)}\n\n`;
    for (const response of this.listeners) response.write(data);
  }

  fail(error) {
    for (const request of this.pending.values()) request.reject(error);
    this.pending.clear();
    this.broadcast({
      method: "gateway/error",
      params: { message: error.message },
    });
  }
}

class RpcError extends Error {
  constructor(error) {
    super(error.message ?? "Codex request failed");
    this.code = error.code;
    this.data = error.data;
  }
}

function resolveCodexExecutable() {
  if (process.env.CODEX_BIN) return process.env.CODEX_BIN;
  const local = resolve(
    here,
    "..",
    "..",
    "codex-rs",
    "target",
    "debug",
    process.platform === "win32" ? "codex.exe" : "codex",
  );
  return existsSync(local) ? local : "codex";
}

const bridge = new AppServerBridge();
const server = createServer(async (request, response) => {
  try {
    const url = new URL(request.url ?? "/", `http://${request.headers.host}`);
    if (request.method === "GET" && url.pathname === "/events") {
      return openEventStream(url, request, response);
    }
    if (request.method === "GET" && url.pathname === "/health") {
      await bridge.ready;
      return json(response, 200, { ready: true, authMode: "chatgpt" });
    }
    if (request.method === "POST" && url.pathname === "/rpc") {
      authorize(request);
      const { method, params } = await readJson(request);
      if (typeof method !== "string" || !method)
        throw new HttpError(400, "method is required");
      return json(response, 200, {
        result: await bridge.request(method, params ?? {}),
      });
    }
    if (request.method === "POST" && url.pathname === "/reply") {
      authorize(request);
      const message = await readJson(request);
      if (!Object.hasOwn(message, "id"))
        throw new HttpError(400, "reply id is required");
      bridge.reply(message);
      return json(response, 202, { accepted: true });
    }
    if (request.method === "GET") return serveStatic(url.pathname, response);
    throw new HttpError(404, "not found");
  } catch (error) {
    const statusCode = error instanceof HttpError ? error.statusCode : 500;
    json(response, statusCode, {
      error: {
        message: error.message,
        code: error.code ?? null,
        data: error.data ?? null,
      },
    });
  }
});

function openEventStream(url, request, response) {
  if (url.searchParams.get("token") !== sessionToken)
    throw new HttpError(403, "forbidden");
  response.writeHead(200, {
    "Cache-Control": "no-cache, no-transform",
    "Connection": "keep-alive",
    "Content-Type": "text/event-stream",
    "X-Accel-Buffering": "no",
  });
  response.write(
    `data: ${JSON.stringify({ method: "gateway/connected", params: {} })}\n\n`,
  );
  const unsubscribe = bridge.subscribe(response);
  request.once("close", unsubscribe);
}

function authorize(request) {
  if (request.headers["x-stateful-session"] !== sessionToken) {
    throw new HttpError(403, "forbidden");
  }
}

async function readJson(request) {
  const chunks = [];
  let size = 0;
  for await (const chunk of request) {
    size += chunk.length;
    if (size > 1024 * 1024) throw new HttpError(413, "request is too large");
    chunks.push(chunk);
  }
  try {
    return JSON.parse(Buffer.concat(chunks).toString("utf8"));
  } catch {
    throw new HttpError(400, "request body must be JSON");
  }
}

async function serveStatic(pathname, response) {
  const requested = pathname === "/" ? "/index.html" : pathname;
  const path = resolve(publicRoot, `.${decodeURIComponent(requested)}`);
  if (relative(publicRoot, path).startsWith("..") || !(await isFile(path))) {
    throw new HttpError(404, "not found");
  }
  let body = await readFile(path);
  if (extname(path) === ".html") {
    body = Buffer.from(
      body
        .toString("utf8")
        .replaceAll("__STATEFUL_SESSION_TOKEN__", sessionToken),
    );
  }
  response.writeHead(200, {
    "Cache-Control": "no-store",
    "Content-Type": mimeTypes.get(extname(path)) ?? "application/octet-stream",
    "Content-Security-Policy":
      "default-src 'self'; connect-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; base-uri 'none'; frame-ancestors 'none'",
  });
  response.end(body);
}

async function isFile(path) {
  try {
    return (await stat(path)).isFile();
  } catch {
    return false;
  }
}

function json(response, statusCode, body) {
  response.writeHead(statusCode, {
    "Cache-Control": "no-store",
    "Content-Type": "application/json; charset=utf-8",
  });
  response.end(JSON.stringify(body));
}

class HttpError extends Error {
  constructor(statusCode, message) {
    super(message);
    this.statusCode = statusCode;
  }
}

server.listen(port, host, () => {
  console.log(`Stateful Codex: http://${host}:${port}`);
});

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.once(signal, () => {
    bridge.child?.kill();
    server.close(() => process.exit(0));
  });
}
