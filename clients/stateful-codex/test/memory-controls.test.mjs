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

test("refreshing a complete entry into an excerpt keeps the draft without submitting", async () => {
  const { controls, state, calls, rpc, storage } = setup();
  await controls.refresh();
  controls.open("e");
  controls.drafts.edit("e", "My complete correction");
  // The next read now exposes only an excerpt, but the original draft remains open.
  state.memory.items[0].contentTruncated = true;
  await controls.refresh();
  assert.equal(state.memory.items[0].contentTruncated, true);
  await assert.rejects(controls.correct("e"), /Read the complete entry/);
  assert.deepEqual(calls.map((call) => call.method), ["statefulMemory/read", "statefulMemory/read"]);
  const reloaded = createMemoryControls({ state, rpc, storage });
  assert.deepEqual(reloaded.drafts.correction("e"), { baseRevision: 2, content: "My complete correction" });
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

test("addition retries keep their action but changed words survive an older delayed save", async () => {
  const state = { projectId: "p", threadId: "t" };
  const calls = [];
  let finish;
  let attempt = 0;
  const controls = createMemoryControls({ state, newId: () => `a${calls.length}`, rpc: async (method, params) => {
    assert.equal(checkRpcParams(method, params), null);
    calls.push({ method, params });
    if (++attempt === 1) throw new Error("lost response");
    if (attempt === 2) return new Promise((resolve) => { finish = resolve; });
    return { outcome: "added" };
  } });
  controls.drafts.editAddition("kind", "decision");
  controls.drafts.editAddition("content", "Use mth.");
  controls.drafts.editAddition("reason", "Avoid confusion");
  await assert.rejects(controls.add(), /lost response/);
  const pending = controls.add();
  controls.drafts.editAddition("content", "Use mo.");
  finish({ outcome: "alreadyDone" });
  await pending;
  assert.equal(controls.drafts.addition().content, "Use mo.");
  assert.deepEqual(calls.map((call) => call.params.clientActionId), ["a0", "a0"]);
  assert.deepEqual(calls[0], { method: "statefulMemory/add", params: { threadId: "t", expectedProjectId: "p",
    kind: "decision", content: "Use mth.", reason: "Avoid confusion", clientActionId: "a0", backgroundSection: true } });
  await controls.add();
  assert.deepEqual([calls[2].params.clientActionId, controls.drafts.addition().content], ["a2", ""]);
});

test("decision limits include the reason and addition drafts are never shortened", async () => {
  const { controls, calls } = setup();
  controls.drafts.editAddition("kind", "decision");
  controls.drafts.editAddition("content", "é".repeat(990));
  controls.drafts.editAddition("reason", "é".repeat(10));
  await assert.rejects(controls.add(), /UTF-8 bytes/);
  assert.equal(controls.drafts.addition().reason, "é".repeat(10));
  assert.deepEqual(calls, []);
});

test("paging continues the cursor, replays opened pages on refresh and recovers changed revisions", async () => {
  const state = { projectId: "p", threadId: "t" };
  const cursors = [];
  let changed = false;
  let revision = 1;
  const controls = createMemoryControls({ state, rpc: async (method, params) => {
    assert.equal(checkRpcParams(method, params), null);
    cursors.push(params.cursor);
    if (changed && params.cursor) {
      changed = false;
      revision = 2;
      throw Object.assign(new Error("project memory changed since this page was read"), { code: -32602 });
    }
    return { projectId: "p", data: [{ entryId: params.cursor ? `second-${revision}` : `first-${revision}` }],
      nextCursor: params.cursor ? null : `p:${revision}:50` };
  } });
  await controls.refresh();
  changed = true;
  await controls.more();
  assert.deepEqual(cursors, [null, "p:1:50", null, "p:2:50"]);
  assert.deepEqual(state.memory.items, [{ entryId: "first-2" }, { entryId: "second-2" }]);
  cursors.length = 0;
  await controls.refresh();
  assert.deepEqual(cursors, [null, "p:2:50"]);
});

test("a late old read cannot replace a newer memory list", async () => {
  const state = { projectId: "p", threadId: "t" };
  const pending = [];
  const controls = createMemoryControls({ state, rpc: () => new Promise((resolve) => pending.push(resolve)) });
  const old = controls.refresh();
  const latest = controls.refresh();
  pending[1]({ projectId: "p", data: [{ entryId: "new" }], nextCursor: null });
  await latest;
  pending[0]({ projectId: "p", data: [{ entryId: "old" }], nextCursor: null });
  await old;
  assert.deepEqual(state.memory.items, [{ entryId: "new" }]);
});

test("a response naming another project is unavailable rather than displayed", async () => {
  const state = { projectId: "p", threadId: "t" };
  const controls = createMemoryControls({ state, rpc: async () => ({ projectId: "other", data: [{ entryId: "foreign" }] }) });
  await controls.refresh();
  assert.deepEqual(state.memory, { items: [], error: "Memory belongs to another project" });
});
