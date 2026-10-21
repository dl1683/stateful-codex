import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import test from "node:test";

import { renderMemory } from "../public/memory-view.mjs";

test("memory controls match the reviewed render snapshot", async () => {
  const html = renderMemory({
    memory: { items: [
      { entryId: "rule", section: "userRule", revision: 3, content: "Current words", replaces: [] },
      { entryId: "long", section: "knowledge", revision: 1, content: "Excerpt", contentTruncated: true },
      { entryId: "background", section: "background", revision: 1, content: "I know <Rust>." },
      { entryId: "scoped", section: "pendingRule", revision: 1, content: "Old rule", scopeState: "unsupported" },
    ], error: null },
    memoryDrafts: {
      rule: { baseRevision: 2, content: "My <unsaved> words" },
      gone: { baseRevision: 1, content: "Orphan draft" },
    },
  });
  const url = new URL("snapshots/memory.html", import.meta.url);
  if (process.env.UPDATE_SNAPSHOTS) await writeFile(url, `${html}\n`);
  assert.equal(`${html}\n`, (await readFile(url, "utf8")).replaceAll("\r\n", "\n"));
  assert.match(html, /changed elsewhere/);
  assert.match(html, /Orphan draft/);
  assert.match(html, /My &lt;unsaved&gt; words/);
  assert.doesNotMatch(html, /data-action="memory-correct" data-entry-id="long"/);
  assert.match(html, /Historical scope is unsupported/);
});
