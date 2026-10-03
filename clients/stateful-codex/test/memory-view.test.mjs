import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import test from "node:test";

import { documentTitle, renderMemory, runStateLabel } from "../public/memory-view.mjs";

const item = (entryId, section, content, extra = {}) => ({
  entryId,
  revision: 1,
  section,
  kind: section === "decision" ? "decision" : "instruction",
  content,
  contentTruncated: false,
  source: "user",
  updatedAt: 1790000000,
  replaces: [],
  ...extra,
});

const memory = {
  items: [
    item("rule-1", "userRule", "From now on, never run the whole test suite."),
    item("pending-1", "pendingRule", "Don't modify any files today."),
    item("agent-1", "unverifiedRule", "Prefer <tabs>.", { source: "agent" }),
    item("stateful-user-background-1", "knowledge", "I know Python well but only a little Rust.", {
      kind: "fact",
    }),
    item("decision-1", "decision", "Months use the symbol mth.", {
      replaces: [{ entryId: "old", content: "Months use the symbol mo.", replacedAt: 1789000000 }],
    }),
  ],
  more: false,
  error: null,
};

test("memory lists sections with forget and correct, and names what a correction does", async () => {
  const html = renderMemory({ memory, memoryEditing: "agent-1" });
  const url = new URL("snapshots/memory.html", import.meta.url);
  if (process.env.UPDATE_SNAPSHOTS) await writeFile(url, `${html}\n`);
  assert.equal(`${html}\n`, await readFile(url, "utf8"));
  assert.match(html, /Correct and apply as your rule/);
  assert.match(html, /Prefer &lt;tabs&gt;\./);
  assert.equal(html.match(/data-action="memory-forget"/g)?.length, 4);
});

test("an unavailable memory says so instead of looking empty", () => {
  assert.match(
    renderMemory({ memory: { items: [], more: false, error: "no project" } }),
    /Project memory is unavailable: no project/,
  );
});

test("the run badge separates the answer from the durable run", () => {
  const run = { mode: "collaborative", status: "running" };
  assert.deepEqual(
    [
      runStateLabel({ run, pendingRequests: [] }),
      runStateLabel({ run, pendingRequests: [], turnInProgress: true }),
      runStateLabel({ run, pendingRequests: [], answeredOnce: true }),
      runStateLabel({ run, pendingRequests: [{ id: 1 }], turnInProgress: true }),
      runStateLabel({ run: { ...run, status: "paused" }, pendingRequests: [] }),
      runStateLabel({ run: { ...run, status: "completed" }, pendingRequests: [] }),
    ],
    [
      "Idle · Collaborative run open",
      "Working · Collaborative run open",
      "Answer finished · Collaborative run open",
      "Waiting for you · Collaborative run open",
      "Idle · Collaborative run paused",
      "Collaborative run completed",
    ],
  );
  assert.deepEqual(
    [documentTitle("Stateful Codex", 0), documentTitle("Stateful Codex", 2)],
    ["Stateful Codex", "(2) Approval needed · Stateful Codex"],
  );
});

test("a shortened entry offers no correction that would drop its tail", () => {
  const html = renderMemory({
    memory: { items: [item("long-1", "userRule", "x".repeat(10), { contentTruncated: true })], more: false, error: null },
    memoryEditing: "long-1",
  });
  assert.doesNotMatch(html, /data-memory-correct|data-action="memory-correct"/);
  assert.match(html, /Too long to correct here/);
});
