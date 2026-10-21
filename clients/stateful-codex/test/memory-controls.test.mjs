import assert from "node:assert/strict";
import test from "node:test";

import { checkRpcParams } from "../gateway-policy.mjs";
import { createMemoryControls } from "../public/memory-controls.mjs";

function setup() {
  const state = { projectId: "p", threadId: "t" };
  let entries = [{ entryId: "e", revision: 2, content: "Original", section: "userRule" }];
  const calls = [];
  const rpc = async (method, params) => {
    assert.equal(checkRpcParams(method, params), null);
    calls.push({ method, params });
    if (params.expectedProjectId !== "p") throw new Error("wrong project; nothing changed");
    if (method === "statefulMemory/read") return { projectId: "p", data: entries, nextCursor: null };
    if (params.expectedRevision !== entries[0]?.revision) throw new Error("stale revision; nothing changed");
    if (method === "statefulMemory/forget") { entries = []; return { entryId: "e", revision: 3 }; }
    entries = [{ ...entries[0], entryId: "successor", revision: 1, content: params.content }];
    return { item: entries[0] };
  };
  const values = new Map();
  const storage = { getItem: (key) => values.get(key), setItem: (key, value) => values.set(key, value) };
  return { state, rpc, storage, calls, controls: createMemoryControls({ state, rpc, storage }) };
}

test("memory uses current project binding, revision and background-section contracts", async () => {
  const { controls, state, calls } = setup();
  await controls.refresh();
  controls.open("e");
  controls.drafts.edit("e", "Corrected");
  await controls.correct("e");
  assert.deepEqual(calls, [
    { method: "statefulMemory/read", params: { threadId: "t", expectedProjectId: "p", cursor: null, limit: 50, backgroundSection: true } },
    { method: "statefulMemory/correct", params: { threadId: "t", expectedProjectId: "p", entryId: "e", expectedRevision: 2, content: "Corrected", backgroundSection: true } },
  ]);
  assert.equal(controls.drafts.correction("e"), null);
  await controls.refresh();
  await controls.forget("successor", state.memory.items[0].revision);
  assert.deepEqual(calls.at(-1), { method: "statefulMemory/forget",
    params: { threadId: "t", expectedProjectId: "p", entryId: "successor", expectedRevision: 1 } });
});

test("wrong-project and stale-revision refusals keep correction text", async () => {
  const { state, controls, rpc, storage } = setup();
  await controls.refresh();
  controls.open("e");
  controls.drafts.edit("e", "My words");
  state.memory.items[0].revision = 3;
  await assert.rejects(controls.correct("e"), /stale revision/);
  assert.deepEqual(controls.drafts.correction("e"), { baseRevision: 2, content: "My words" });
  const wrong = createMemoryControls({ state: { projectId: "other", threadId: "t" }, rpc, storage });
  await wrong.refresh();
  await assert.rejects(wrong.forget("e", 3), /wrong project/);
});

test("UTF-8 overflow and excerpts never submit a correction", async () => {
  const { controls, state, calls } = setup();
  await controls.refresh();
  state.memory.items[0].contentTruncated = true;
  assert.throws(() => controls.open("e"), /excerpt/);
  state.memory.items[0].contentTruncated = false;
  controls.open("e");
  const content = "é".repeat(1001);
  controls.drafts.edit("e", content);
  await assert.rejects(controls.correct("e"), /UTF-8 bytes/);
  assert.equal(controls.drafts.correction("e").content, content);
  assert.equal(calls.length, 1);
});

test("typing during a delayed correction survives the save and reload", async () => {
  const { state, controls, storage } = setup();
  await controls.refresh();
  let finish;
  const delayed = createMemoryControls({ state, storage, rpc: () => new Promise((resolve) => { finish = resolve; }) });
  delayed.open("e");
  delayed.drafts.edit("e", "Submitted");
  const saving = delayed.correct("e");
  delayed.drafts.edit("e", "Typed while saving");
  finish({ item: { entryId: "successor", content: "Submitted" } });
  await saving;
  const reloaded = createMemoryControls({ state, storage, rpc: async () => ({ projectId: "p", data: [], nextCursor: null }) });
  await reloaded.refresh();
  assert.deepEqual(reloaded.drafts.correction("e"), { baseRevision: 2, content: "Typed while saving" });
});
