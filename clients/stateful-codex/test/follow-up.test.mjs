import assert from "node:assert/strict";
import test from "node:test";

import {
  readPendingFollowUp,
  resumePendingFollowUp,
  sendFirstTurn,
  startFollowUp,
} from "../public/follow-up.mjs";
import { workspaceFixture } from "./workspace-fixture.mjs";

const SCOPE = { projectId: "project-1", threadId: "thread-1" };

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
    ...SCOPE,
    confirmedSteering: [{ id: "steer-old" }],
    steeringError: "old error",
    runGeneration: 0,
  };
  state.run = { ...state.run, status: "completed" };
  return state;
}

// A fake app-server: runs are keyed by idempotency key (as the server derives run IDs), the
// thread may be unloaded, and recorded messages carry the time they started.
function fakeServer({ threadStatus = "idle", history = [], failOnce = {}, pageSize } = {}) {
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
      case "statefulRun/start":
        if (!runs.has(params.idempotencyKey)) {
          runs.set(params.idempotencyKey, {
            id: `run-${runs.size + 2}`,
            goal: params.goal,
            mode: params.mode,
            budget: params.budget,
            status: "running",
          });
        }
        return { run: runs.get(params.idempotencyKey) };
      case "turn/start":
        if (threadStatus === "notLoaded") throw new Error("thread not found");
        history.push({ type: "userMessage", text: params.input[0].text, startedAtMs: Date.now() });
        return { turn: { id: "turn" } };
      case "thread/items/list": {
        const newestFirst = [...history].reverse();
        const offset = Number(params.cursor ?? 0);
        const limit = pageSize ?? params.limit;
        const page = newestFirst.slice(offset, offset + limit);
        return {
          data: page.map((entry) => ({
            item:
              entry.type === "userMessage"
                ? { type: "userMessage", content: [{ type: "text", text: entry.text }] }
                : { type: entry.type },
            startedAtMs: entry.startedAtMs,
          })),
          nextCursor: offset + limit < newestFirst.length ? String(offset + limit) : null,
        };
      }
      default:
        throw new Error(`unexpected ${method}`);
    }
  };
  return { rpc, calls, runs, history };
}

const userTexts = (history) =>
  history.filter((entry) => entry.type === "userMessage").map((entry) => entry.text);

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

  // A turn never dispatched before is simply sent: no history check can suppress it.
  assert.deepEqual(server.calls, ["thread/read", "statefulRun/start", "thread/read", "turn/start"]);
  assert.equal(renders, 1);
  assert.deepEqual(userTexts(server.history), ["Rename --month to --period"]);
  assert.deepEqual([...server.runs.values()][0].budget, { maxContinuations: 24, maxElapsedSeconds: 14400 });
  assert.equal(state.runGeneration, 1);
  assert.deepEqual(
    [state.run.id, state.obligations, state.steering, state.confirmedSteering, state.steeringError],
    ["run-2", [], [], [], null],
  );
  assert.equal(readPendingFollowUp(storage, SCOPE), null);
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

test("an identical earlier message never stands in for a new instruction", async () => {
  const server = fakeServer({
    history: [{ type: "userMessage", text: "Run the tests", startedAtMs: Date.now() - 1000 }],
  });
  const storage = memoryStorage();

  await sendFirstTurn({ rpc: server.rpc, storage, threadId: "thread-1", run: { id: "run-9" }, text: "Run the tests" });

  assert.deepEqual(userTexts(server.history), ["Run the tests", "Run the tests"]);
});

test("a lost start response is recovered by replaying the same request, not a second run", async () => {
  const state = closedWorkspace();
  const storage = memoryStorage();
  const server = fakeServer({
    failOnce: { "statefulRun/start": { message: "network error", afterEffect: true } },
  });

  await assert.rejects(
    startFollowUp({ state, rpc: server.rpc, storage, goal: "next step", mode: "autonomous" }),
    /network error/,
  );
  assert.equal(server.runs.size, 1);
  assert.equal(readPendingFollowUp(storage, SCOPE).goal, "next step");

  // A retry (or a reload, which resumes the saved follow-up) reuses the saved key.
  await resumePendingFollowUp({ state, rpc: server.rpc, storage });
  assert.equal(server.runs.size, 1);
  assert.equal(state.run.id, "run-2");
  assert.deepEqual(userTexts(server.history), ["next step"]);
});

test("a first turn whose response was lost is not sent twice, even after long activity", async () => {
  const state = closedWorkspace();
  const storage = memoryStorage();
  const server = fakeServer({
    failOnce: { "turn/start": { message: "connection reset", afterEffect: true } },
    pageSize: 10,
  });

  await assert.rejects(
    startFollowUp({ state, rpc: server.rpc, storage, goal: "next step", mode: "autonomous" }),
    /connection reset/,
  );
  // The accepted turn did plenty of work before the page was reloaded.
  for (let index = 0; index < 35; index += 1) {
    server.history.push({ type: "commandExecution", startedAtMs: Date.now() });
  }
  await resumePendingFollowUp({ state, rpc: server.rpc, storage });

  assert.deepEqual(userTexts(server.history), ["next step"]);
  assert.equal(storage.getItem("stateful-initial-turn-sent"), "run-2");
  assert.equal(readPendingFollowUp(storage, SCOPE), null);
});

test("an unknown delivery with history too long to check is not resent", async () => {
  const storage = memoryStorage({
    "stateful-turn-dispatch": JSON.stringify({ runId: "run-1", at: Date.now() }),
  });
  const history = Array.from({ length: 600 }, () => ({ type: "reasoning", startedAtMs: Date.now() }));
  const server = fakeServer({ history });

  await assert.rejects(
    sendFirstTurn({ rpc: server.rpc, storage, threadId: "thread-1", run: { id: "run-1" }, text: "Start" }),
    /isn't certain whether the first instruction reached the agent/,
  );
  assert.ok(!server.calls.includes("turn/start"));
});

test("an unloaded thread is resumed before the first turn", async () => {
  const server = fakeServer({ threadStatus: "notLoaded" });
  const storage = memoryStorage();

  await sendFirstTurn({ rpc: server.rpc, storage, threadId: "thread-1", run: { id: "run-1" }, text: "Start" });

  assert.deepEqual(server.calls, ["thread/read", "thread/resume", "turn/start"]);
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
  const refused = await startFollowUp({
    state,
    rpc: server.rpc,
    storage,
    goal: "second idea",
    mode: "autonomous",
  }).catch((error) => error);

  assert.match(refused.message, /An earlier follow-up \("first idea"\) had not finished starting/);
  assert.equal(refused.refusedText, "second idea");
  assert.equal(state.run.goal, "first idea");
  assert.deepEqual(userTexts(server.history), ["first idea"]);
  assert.equal(readPendingFollowUp(storage, SCOPE), null);
});

test("a follow-up saved by another workspace never runs here", async () => {
  const storage = memoryStorage({
    "stateful-follow-up": JSON.stringify({
      projectId: "project-other",
      threadId: "thread-other",
      goal: "foreign work",
      mode: "autonomous",
      budget: { maxContinuations: 1, maxElapsedSeconds: 60 },
      key: "key-1",
    }),
  });
  const server = fakeServer();

  assert.equal(
    await resumePendingFollowUp({ state: closedWorkspace(), rpc: server.rpc, storage }),
    false,
  );
  assert.deepEqual(server.calls, []);
  assert.equal(storage.getItem("stateful-follow-up"), null);
});
