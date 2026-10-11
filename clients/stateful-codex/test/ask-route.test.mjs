import assert from "node:assert/strict";
import test from "node:test";

import {
  ASK_SENT_KEY,
  AskRefused,
  askRefusal,
  createAskGate,
  isAskWorkspace,
  sendInitialQuestion,
} from "../public/ask-route.mjs";

function memoryStorage() {
  const values = new Map();
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, String(value)),
    removeItem: (key) => values.delete(key),
  };
}

test("Ask is chosen at setup and needs no run", () => {
  assert.deepEqual(
    ["ask", "autonomous", "collaborative", "socratic", null].map(isAskWorkspace),
    [true, false, false, false, false],
  );
});

test("the setup question is sent once across refresh and reconnect", async () => {
  const storage = memoryStorage();
  const sent = [];
  const open = () =>
    sendInitialQuestion({
      storage,
      threadId: "thread-1",
      question: "What did we decide about the database?",
      sendTurn: async (text) => sent.push(text),
    });
  // First boot sends; a refresh or reconnect reopens the same Ask thread without resending.
  assert.deepEqual([await open(), await open(), await open()], [true, false, false]);
  assert.deepEqual(sent, ["What did we decide about the database?"]);
  // A new Ask thread from setup sends its own question.
  assert.equal(
    await sendInitialQuestion({
      storage,
      threadId: "thread-2",
      question: "What does src/app.py do?",
      sendTurn: async (text) => sent.push(text),
    }),
    true,
  );
  assert.equal(storage.getItem(ASK_SENT_KEY), "thread-2");
});

test("a refresh during submission never resends; a failed send can be retried", async () => {
  const storage = memoryStorage();
  let release;
  const pending = new Promise((resolve) => {
    release = resolve;
  });
  const sent = [];
  const first = sendInitialQuestion({
    storage,
    threadId: "thread-1",
    question: "Q",
    sendTurn: async (text) => {
      sent.push(text);
      await pending;
    },
  });
  // The page reloads while turn/start is in flight.
  assert.equal(
    await sendInitialQuestion({
      storage,
      threadId: "thread-1",
      question: "Q",
      sendTurn: async (text) => sent.push(text),
    }),
    false,
  );
  release();
  assert.equal(await first, true);
  assert.deepEqual(sent, ["Q"]);

  const failing = memoryStorage();
  await assert.rejects(
    sendInitialQuestion({
      storage: failing,
      threadId: "thread-3",
      question: "Q",
      sendTurn: async () => {
        throw new Error("network down");
      },
    }),
    /network down/,
  );
  assert.equal(failing.getItem(ASK_SENT_KEY), null);
});

test("a thread with an open run cannot be asked run-free", () => {
  assert.deepEqual(
    [
      null,
      { mode: "autonomous", status: "completed" },
      { mode: "autonomous", status: "answered" },
      { mode: "collaborative", status: "running" },
      { mode: "autonomous", status: "blocked" },
    ].map((run) => askRefusal(run) !== null),
    [false, false, false, true, true],
  );
});

// A fake app server: the thread's run (mutable) and every turn/start it received.
function fakeServer(run) {
  const server = { run, turns: [] };
  server.rpc = async (method, params) => {
    if (method === "statefulRun/read") return { run: server.run };
    if (method === "turn/start") {
      server.turns.push(params);
      return { turn: { id: `turn-${server.turns.length}` } };
    }
    throw new Error(`unexpected ${method}`);
  };
  return server;
}

test("a refused Ask never submits, before or after a refresh", async () => {
  for (const status of ["running", "paused", "blocked"]) {
    const storage = memoryStorage();
    const server = fakeServer({ mode: "autonomous", status });
    const gate = createAskGate({ rpc: server.rpc, storage, threadId: "work" });
    assert.match(await gate.admit(), /open autonomous run/);
    await assert.rejects(gate.send("What is the capital of France?"), AskRefused);
    // Refresh: a new gate on the same storage keeps the refusal without asking the server.
    const reloaded = createAskGate({
      rpc: server.rpc,
      storage,
      threadId: "work",
    });
    assert.match(reloaded.refusal(), /open autonomous run/);
    await assert.rejects(reloaded.send("Again?"), AskRefused);
    assert.deepEqual(server.turns, [], status);
  }
});

test("an admitted Ask thread that gains a Work run refuses the next send", async () => {
  const storage = memoryStorage();
  const server = fakeServer(null);
  const gate = createAskGate({ rpc: server.rpc, storage, threadId: "ask" });
  assert.equal(await gate.admit(), null);
  await gate.send("What is the capital of France?");
  // Another client starts Work on the same thread.
  server.run = { mode: "collaborative", status: "running" };
  await assert.rejects(gate.send("And of Spain?"), AskRefused);
  assert.deepEqual(
    server.turns.map((turn) => turn.input[0].text),
    ["What is the capital of France?"],
  );
});
