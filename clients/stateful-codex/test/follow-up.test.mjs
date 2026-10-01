import assert from "node:assert/strict";
import test from "node:test";

import {
  readPendingFollowUp,
  resumePendingFollowUp,
  sendFirstTurn,
  startFollowUp,
} from "../public/follow-up.mjs";
import { workspaceFixture } from "./workspace-fixture.mjs";

function memoryStorage(entries = {}) {
  const values = new Map(Object.entries(entries));
  return {
    values,
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, String(value)),
    removeItem: (key) => values.delete(key),
  };
}

function closedWorkspace() {
  const state = {
    ...workspaceFixture(),
    projectId: "project-1",
    threadId: "thread-1",
    confirmedSteering: [{ id: "steer-old" }],
    steeringError: "old error",
    runGeneration: 0,
  };
  state.run = { ...state.run, status: "completed" };
  return state;
}

// A fake app-server: runs are keyed by idempotency key (as the server derives run IDs), the
// thread may be unloaded, and recorded user messages answer the "did it arrive" check.
function fakeServer({ threadStatus = "idle", userMessages = [], failOnce = {} } = {}) {
  const calls = [];
  const runs = new Map();
  const rpc = async (method, params) => {
    calls.push(method);
    if (failOnce[method]) {
      const failure = failOnce[method];
      delete failOnce[method];
      if (failure.afterEffect) await rpc(method, params);
      throw new Error(failure.message);
    }
    switch (method) {
      case "thread/read":
        return { thread: { id: params.threadId, status: { type: threadStatus } } };
      case "thread/resume":
        threadStatus = "idle";
        return { thread: { id: params.threadId } };
      case "statefulRun/start": {
        if (!runs.has(params.idempotencyKey)) {
          runs.set(params.idempotencyKey, {
            id: `run-${runs.size + 2}`,
            goal: params.goal,
            mode: params.mode,
            budget: params.budget,
            status: "running",
            createdAt: 1_790_000_000,
          });
        }
        return { run: runs.get(params.idempotencyKey) };
      }
      case "turn/start":
        if (threadStatus === "notLoaded") throw new Error("thread not found");
        userMessages.push({ text: params.input[0].text, startedAtMs: 1_790_000_001_000 });
        return { turn: { id: "turn" } };
      case "thread/items/list":
        return {
          data: [...userMessages].reverse().map((message) => ({
            item: { type: "userMessage", content: [{ type: "text", text: message.text }] },
            startedAtMs: message.startedAtMs,
          })),
        };
      default:
        throw new Error(`unexpected ${method}`);
    }
  };
  return { rpc, calls, runs, userMessages };
}

test("a follow-up starts a new run on the same thread, then sends it as the first turn", async () => {
  const state = closedWorkspace();
  const storage = memoryStorage({ "stateful-initial-turn-sent": "run-1" });
  const server = fakeServer();
  let renders = 0;

  await startFollowUp({
    state,
    rpc: server.rpc,
    storage,
    goal: "Rename --month to --period",
    mode: "collaborative",
    onRunChanged: () => {
      renders += 1;
      assert.equal(state.run.id, "run-2");
    },
  });

  assert.deepEqual(server.calls, [
    "thread/read",
    "statefulRun/start",
    "thread/read",
    "thread/items/list",
    "turn/start",
  ]);
  assert.equal(renders, 1);
  assert.deepEqual(server.userMessages.map((message) => message.text), ["Rename --month to --period"]);
  assert.deepEqual([...server.runs.values()][0].budget, { maxContinuations: 24, maxElapsedSeconds: 14400 });
  assert.equal(state.runGeneration, 1);
  assert.deepEqual(
    [state.run.id, state.obligations, state.steering, state.confirmedSteering, state.steeringError],
    ["run-2", [], [], [], null],
  );
  assert.equal(readPendingFollowUp(storage), null);
  assert.deepEqual(
    Object.fromEntries([...storage.values].filter(([key]) => key !== "stateful-run-key")),
    {
      "stateful-mode": "collaborative",
      "stateful-goal": "Rename --month to --period",
      "stateful-created-run-id": "run-2",
      "stateful-initial-turn-sent": "run-2",
    },
  );
});

test("a lost start response is recovered by replaying the same request, not a second run", async () => {
  const state = closedWorkspace();
  const storage = memoryStorage();
  const server = fakeServer({
    failOnce: { "statefulRun/start": { message: "network error", afterEffect: true } },
  });
  const follow = () =>
    startFollowUp({ state, rpc: server.rpc, storage, goal: "next step", mode: "autonomous" });

  await assert.rejects(follow(), /network error/);
  assert.equal(server.runs.size, 1);
  assert.equal(readPendingFollowUp(storage).goal, "next step");

  // A retry (or a reload, which resumes the saved follow-up) reuses the saved key.
  await resumePendingFollowUp({ state, rpc: server.rpc, storage });
  assert.equal(server.runs.size, 1);
  assert.equal(state.run.id, "run-2");
  assert.deepEqual(server.userMessages.map((message) => message.text), ["next step"]);
});

test("a first turn whose response was lost is not sent twice", async () => {
  const state = closedWorkspace();
  const storage = memoryStorage();
  const server = fakeServer({
    failOnce: { "turn/start": { message: "connection reset", afterEffect: true } },
  });

  await assert.rejects(
    startFollowUp({ state, rpc: server.rpc, storage, goal: "next step", mode: "autonomous" }),
    /connection reset/,
  );
  await resumePendingFollowUp({ state, rpc: server.rpc, storage });

  assert.deepEqual(server.userMessages.map((message) => message.text), ["next step"]);
  assert.equal(storage.getItem("stateful-initial-turn-sent"), "run-2");
  assert.equal(readPendingFollowUp(storage), null);
});

test("an unloaded thread is resumed before the first turn", async () => {
  const server = fakeServer({ threadStatus: "notLoaded" });
  const storage = memoryStorage();

  await sendFirstTurn({
    rpc: server.rpc,
    storage,
    threadId: "thread-1",
    run: { id: "run-1", createdAt: 1_790_000_000 },
    text: "Start",
  });

  assert.deepEqual(server.calls, ["thread/read", "thread/resume", "thread/items/list", "turn/start"]);
  assert.equal(storage.getItem("stateful-initial-turn-sent"), "run-1");
});

test("an earlier unconfirmed follow-up is finished before different text is accepted", async () => {
  const state = closedWorkspace();
  const storage = memoryStorage();
  const server = fakeServer({ failOnce: { "statefulRun/start": { message: "timeout" } } });

  await assert.rejects(
    startFollowUp({ state, rpc: server.rpc, storage, goal: "first idea", mode: "autonomous" }),
    /timeout/,
  );
  await assert.rejects(
    startFollowUp({ state, rpc: server.rpc, storage, goal: "second idea", mode: "autonomous" }),
    /An earlier follow-up \("first idea"\) had not finished starting; it has now been started\. Your new text was not sent/,
  );

  assert.equal(state.run.goal, "first idea");
  assert.deepEqual(server.userMessages.map((message) => message.text), ["first idea"]);
  assert.equal(readPendingFollowUp(storage), null);
});
