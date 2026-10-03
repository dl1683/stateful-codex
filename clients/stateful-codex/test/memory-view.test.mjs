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
  const html = renderMemory({
    memory,
    memoryDrafts: { "agent-1": { baseRevision: 1, content: "Prefer <tabs>." } },
  });
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
      runStateLabel({ run, pendingRequests: [], execution: { phase: "idle" } }),
      runStateLabel({ run, pendingRequests: [], execution: { phase: "working" } }),
      runStateLabel({ run, pendingRequests: [], execution: { phase: "finished" } }),
      runStateLabel({ run, pendingRequests: [{ id: 1 }], execution: { phase: "working" } }),
      runStateLabel({ run: { ...run, status: "paused" }, pendingRequests: [], execution: { phase: "idle" } }),
      runStateLabel({ run: { ...run, mode: "autonomous" }, pendingRequests: [], execution: { phase: "unknown" } }),
      runStateLabel({ run: { ...run, status: "completed" }, pendingRequests: [], execution: { phase: "finished" } }),
    ],
    [
      "Checking what is running… · Collaborative run stays open between answers",
      "Not working right now · Collaborative run stays open between answers",
      "Working · Collaborative run stays open between answers",
      "Answer finished · Collaborative run stays open between answers",
      "Waiting for you · Collaborative run stays open between answers",
      "Not working right now · Collaborative run paused",
      "Not sure whether work is running · Autonomous run open; it may continue on its own",
      "Answer finished · Collaborative run completed",
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
    memoryDrafts: { "long-1": { baseRevision: 1, content: "x" } },
  });
  assert.doesNotMatch(html, /data-memory-correct|data-action="memory-correct"/);
  assert.match(html, /Too long to correct here/);
});

test("drafts survive re-rendering, conflicts keep the text, and orphaned drafts stay visible", () => {
  const html = renderMemory({
    memory: {
      items: [
        item("rule-1", "userRule", "Never commit.", { revision: 3, scopeTitle: "Ground rules for this investigation" }),
        item("note-1", "knowledge", "Relayed by the user.", { attributedTo: "Priya" }),
      ],
      cursor: "next",
      pages: 1,
      error: null,
    },
    memoryDrafts: {
      "rule-1": { baseRevision: 2, content: "Never commit or push." },
      "gone-1": { baseRevision: 1, content: "My unsaved words." },
    },
    memoryAddition: { kind: "decision", content: "Months use mth.", reason: "minutes" },
  });
  assert.match(html, /<textarea name="content" data-entry-id="rule-1" data-revision="2" aria-label="Corrected text" required>Never commit or push\.<\/textarea>/);
  assert.match(html, /data-revision="2"/);
  assert.match(html, /changed elsewhere since you began/);
  assert.match(html, /My unsaved words\./);
  assert.match(html, /Only in the investigation: Ground rules for this investigation/);
  assert.match(html, /Priya's words you passed on, not your rule/);
  assert.match(html, /data-action="memory-more"/);
  assert.match(html, /<option value="decision" selected>/);
  assert.match(html, /Months use mth\./);
});
