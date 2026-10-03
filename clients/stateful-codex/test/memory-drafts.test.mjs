import assert from "node:assert/strict";
import test from "node:test";

import { createDrafts } from "../public/memory-drafts.mjs";

function memoryStorage() {
  const values = new Map();
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
  };
}

test("a correction draft survives a reload in the same tab until it is closed", () => {
  const storage = memoryStorage();
  const first = createDrafts(storage, "thread-1");
  first.open("rule-1", 2, "Never commit.");
  first.edit("rule-1", "Never commit or push.");
  // Opening again (a re-render) keeps what was typed.
  first.open("rule-1", 3, "Never commit.");
  first.editAddition("content", "Months use mth.");
  const reloaded = createDrafts(storage, "thread-1");
  const other = createDrafts(storage, "thread-2");
  assert.deepEqual(
    [reloaded.correction("rule-1"), reloaded.addition().content, other.correction("rule-1")],
    [{ baseRevision: 2, content: "Never commit or push." }, "Months use mth.", null],
  );
  reloaded.close("rule-1");
  reloaded.clearAddition();
  assert.deepEqual(
    [createDrafts(storage, "thread-1").correction("rule-1"), createDrafts(storage, "thread-1").addition().content],
    [null, ""],
  );
});
