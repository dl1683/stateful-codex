import assert from "node:assert/strict";
import { mkdir, mkdtemp, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { gradeAttempt } from "../graders/tui-evidence.mjs";
import { buildScorecard, writeScorecard } from "../harness/scorecard.mjs";

test("grades evidence observations and writes JSON plus Markdown without a synthetic score", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "scbench-bundle-"));
  const evidence = path.join(root, "evidence"); await mkdir(evidence);
  for (const file of ["transcript.txt", "actions.jsonl", "approvals.jsonl", "rollout.jsonl", "workspace.patch"]) await writeFile(path.join(evidence, file), "");
  await writeFile(path.join(evidence, "turns.json"), JSON.stringify([{ status: "idle" }]));
  await writeFile(path.join(evidence, "project-state.json"), JSON.stringify({ available: false }));
  await writeFile(path.join(evidence, "store-inventory.json"), "{}");
  const scenario = { id: "surface.tui.test", conversation: { turns: ["next"] } };
  const attempt = { scenarioId: scenario.id, repetition: 1, exitReason: "processExited", retryClassification: "none" };
  const grade = await gradeAttempt({ attemptRoot: root, scenario, attempt });
  const scorecard = buildScorecard({ manifest: { codex: "codex.exe", startedAt: "a", finishedAt: "b", attempts: [attempt] }, plan: { attempts: 1 }, grades: [grade] });
  await writeScorecard(root, scorecard);
  assert.equal(scorecard.findings.some(({ id }) => id === "userMessagesDelivered"), true);
  assert.equal(Object.hasOwn(scorecard, "score"), false);
});
