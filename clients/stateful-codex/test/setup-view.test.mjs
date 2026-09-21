import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { renderSetup } from "../public/setup-view.mjs";

test("setup screen matches the reviewed snapshot", async () => {
  const actual = renderSetup({
    busy: false,
    error: null,
    account: { type: "chatgpt" },
    projects: [
      {
        id: "project-1",
        name: "Deep investigation",
        roots: [{ path: "C:/work/investigation" }],
      },
    ],
    threads: [
      {
        id: "thread-1",
        name: "Primary analysis",
        preview: "Find the decisive fact",
      },
    ],
    projectId: "project-1",
    projectName: "",
    rootPath: "",
    threadAction: "continue",
    threadId: "thread-1",
    mode: "autonomous",
    goal: "Resolve the central contradiction and preserve the evidence.",
    maxContinuations: 24,
    maxElapsedSeconds: 14400,
  });
  const expected = await readFile(
    new URL("snapshots/setup.html", import.meta.url),
    "utf8",
  );
  assert.equal(`${actual.trim()}\n`, expected);
});

test("setup requires the cached Codex ChatGPT login", () => {
  const actual = renderSetup({
    busy: false,
    error: null,
    account: null,
    projects: [],
    threads: [],
    projectId: "new",
    projectName: "",
    rootPath: "",
    threadAction: "create",
    threadId: "",
    mode: "collaborative",
    goal: "",
    maxContinuations: 24,
    maxElapsedSeconds: 14400,
  });

  assert.match(actual, /Run codex login in a terminal/);
  assert.match(actual, /<button class="primary" type="submit" disabled>/);
});
