import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import {
  findRolloutPath,
  resolveRolloutPath,
} from "../eval/rollout-path.mjs";

test("pins one exact nested rollout and prefers the recorded path", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "stateful-rollout-path-"));
  const threadId = "thread-1";
  const rollout = path.join(root, "2026", "09", `rollout-date-${threadId}.jsonl`);
  try {
    await mkdir(path.dirname(rollout), { recursive: true });
    await writeFile(rollout, "{}\n");
    assert.equal(await findRolloutPath(root, threadId), rollout);
    assert.equal(
      await resolveRolloutPath({ threadId, rolloutPath: rollout }),
      rollout,
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("rejects ambiguous rollout discovery", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "stateful-rollout-path-"));
  const threadId = "thread-1";
  try {
    await Promise.all([
      writeFile(path.join(root, `rollout-a-${threadId}.jsonl`), "{}\n"),
      writeFile(path.join(root, `rollout-b-${threadId}.jsonl`), "{}\n"),
    ]);
    await assert.rejects(findRolloutPath(root, threadId), /found 2/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
