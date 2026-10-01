import assert from "node:assert/strict";
import test from "node:test";

import { startFollowUp } from "../public/follow-up.mjs";
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
  };
  state.run = { ...state.run, status: "completed" };
  return state;
}

test("a follow-up starts a new run on the same thread, then sends it as the first turn", async () => {
  const state = closedWorkspace();
  const storage = memoryStorage({ "stateful-initial-turn-sent": "run-1" });
  const calls = [];
  const rpc = async (method, params) => {
    calls.push([method, { ...params, idempotencyKey: typeof params.idempotencyKey }]);
    return { run: { id: "run-2", mode: params.mode, status: "running", budget: params.budget } };
  };
  const sent = [];
  const sendTurn = async (text) => {
    sent.push([text, storage.getItem("stateful-initial-turn-sent")]);
  };

  await startFollowUp({ state, rpc, storage, goal: "Rename --month to --period", mode: "collaborative", sendTurn });

  assert.deepEqual(calls, [
    [
      "statefulRun/start",
      {
        projectId: "project-1",
        threadId: "thread-1",
        goal: "Rename --month to --period",
        mode: "collaborative",
        budget: { maxContinuations: 24, maxElapsedSeconds: 14400 },
        idempotencyKey: "string",
      },
    ],
  ]);
  // The first turn is recorded as unsent until it is sent, so a reload retries it.
  assert.deepEqual(sent, [["Rename --month to --period", null]]);
  assert.equal(state.run.id, "run-2");
  assert.deepEqual(
    [state.obligations, state.steering, state.confirmedSteering, state.steeringError],
    [[], [], [], null],
  );
  assert.deepEqual(Object.fromEntries([...storage.values].filter(([key]) => key !== "stateful-run-key")), {
    "stateful-mode": "collaborative",
    "stateful-goal": "Rename --month to --period",
    "stateful-created-run-id": "run-2",
    "stateful-initial-turn-sent": "run-2",
  });
});

test("a refused follow-up leaves the closed outcome in place", async () => {
  const state = closedWorkspace();
  const storage = memoryStorage();
  const rpc = async () => {
    throw new Error("thread already has active Stateful run run-9");
  };

  await assert.rejects(
    startFollowUp({ state, rpc, storage, goal: "next", mode: "autonomous", sendTurn: async () => assert.fail("sent") }),
    /active Stateful run/,
  );
  assert.equal(state.run.id, "run-1");
  assert.equal(storage.getItem("stateful-created-run-id"), null);
});
