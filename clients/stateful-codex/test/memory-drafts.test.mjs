import assert from "node:assert/strict";
import test from "node:test";

import { MAX_ENTRY_BYTES, createDrafts, fitsEntry } from "../public/memory-drafts.mjs";

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

test("a draft longer than an entry is kept whole and judged in UTF-8 bytes", () => {
  const drafts = createDrafts(memoryStorage(), "thread-1");
  const long = "a".repeat(MAX_ENTRY_BYTES + 1);
  drafts.editAddition("content", long);
  // 1,000 two-byte characters fit; 1,001 do not.
  assert.deepEqual(
    [
      drafts.addition().content.length,
      fitsEntry(long),
      fitsEntry("é".repeat(MAX_ENTRY_BYTES / 2)),
      fitsEntry("é".repeat(MAX_ENTRY_BYTES / 2 + 1)),
    ],
    [MAX_ENTRY_BYTES + 1, false, true, false],
  );
});

test("changing an addition after a failed save makes it a new action", () => {
  const drafts = createDrafts(memoryStorage(), "thread-1");
  drafts.editAddition("content", "Months use mth.");
  drafts.editAddition("actionId", "action-1");
  // A retry of the same words keeps the action; other words start a new one.
  const retried = drafts.addition().actionId;
  drafts.editAddition("content", "Months use mo.");
  assert.deepEqual([retried, drafts.addition().actionId], ["action-1", undefined]);
});
