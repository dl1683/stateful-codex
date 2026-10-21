import assert from "node:assert/strict";
import test from "node:test";

import { checkRpcParams } from "../gateway-policy.mjs";
import { createMemoryControls } from "../public/memory-controls.mjs";
import { createGatewayFixture } from "./gateway-fixture.mjs";

for (const mutation of ["forget", "correct"]) {
  test(`a lost Add response retried after ${mutation} keeps the unsaved draft without restoring memory`, async () => {
    const bridge = createGatewayFixture("C:/disposable/replay-project");
    const state = { projectId: "fixture-project", threadId: "fixture-thread" };
    const values = new Map();
    const storage = { getItem: (key) => values.get(key), setItem: (key, value) => values.set(key, value) };
    let added;
    const rpc = async (method, params) => {
      assert.equal(checkRpcParams(method, params), null);
      const result = await bridge.request(method, params);
      if (method === "statefulMemory/add" && !added) {
        added = result.item;
        throw new Error("lost response");
      }
      return result;
    };
    const controls = createMemoryControls({ state, rpc, storage, newId: () => "lost-add" });
    controls.drafts.editAddition("kind", "decision");
    controls.drafts.editAddition("content", "Use mo.");
    controls.drafts.editAddition("reason", "Avoid ambiguity");
    await assert.rejects(controls.add(), /lost response/);
    const draft = structuredClone(controls.drafts.addition());
    await controls.refresh();
    if (mutation === "forget") {
      await controls.forget(added.entryId, added.revision);
    } else {
      controls.open(added.entryId);
      controls.drafts.edit(added.entryId, "Use month.");
      await controls.correct(added.entryId);
    }
    await controls.refresh();
    const before = structuredClone(state.memory);
    const notice = state.notice;
    // A page reload must not settle the pending action either.
    const reloaded = createMemoryControls({ state, rpc, storage });
    await assert.rejects(reloaded.add(), /entry is retired; nothing was restored/);
    assert.deepEqual(reloaded.drafts.addition(), draft);
    assert.equal(state.notice, notice);
    await reloaded.refresh();
    assert.deepEqual(state.memory, before);
    assert.equal(state.memory.items.some((item) => item.entryId === added.entryId), false);
    if (mutation === "correct") assert.equal(state.memory.items[0].content, "Use month.");
    assert.deepEqual(bridge.calls.filter((call) => call.method === "statefulMemory/add").map((call) => call.params), [
      { threadId: state.threadId, expectedProjectId: state.projectId, kind: "decision",
        content: "Use mo.", reason: "Avoid ambiguity", clientActionId: "lost-add", backgroundSection: true },
      { threadId: state.threadId, expectedProjectId: state.projectId, kind: "decision",
        content: "Use mo.", reason: "Avoid ambiguity", clientActionId: "lost-add", backgroundSection: true },
    ]);
  });
}
