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

test("ignores startup loading frames and environment context when grading a rollout", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "scbench-rollout-"));
  const evidence = path.join(root, "evidence"); await mkdir(evidence);
  for (const file of ["actions.jsonl", "approvals.jsonl", "transcript.jsonl", "workspace.patch"]) await writeFile(path.join(evidence, file), "{}\n");
  await writeFile(path.join(evidence, "transcript.txt"), "model: loading\nnormal final screen\n");
  await writeFile(path.join(evidence, "screen-final.txt"), "Ask Codex to do anything\n");
  await writeFile(path.join(evidence, "turns.json"), JSON.stringify([{ status: "idle" }]));
  await writeFile(path.join(evidence, "project-state.json"), JSON.stringify({ available: false, applicable: false }));
  await writeFile(path.join(evidence, "store-inventory.json"), "{}");
  await writeFile(path.join(evidence, "store-snapshot.json"), JSON.stringify({ consistent: true }));
  await writeFile(path.join(evidence, "sandbox-preflight.json"), JSON.stringify({ writable: true, seeded: true }));
  await writeFile(path.join(evidence, "workspace-before.json"), "[]");
  await writeFile(path.join(evidence, "workspace-after.json"), "[]");
  await writeFile(path.join(evidence, "rollout-source.json"), JSON.stringify({ threadId: "thread", candidates: [{ path: "rollout", threadId: "thread" }] }));
  const environment = { type: "response_item", payload: { role: "user", content: [{ type: "input_text", text: "<environment_context>" }], internal_chat_message_metadata_passthrough: { content_item_kinds: ["environments.environment_context"] } } };
  const user = { type: "response_item", payload: { role: "user", content: [{ type: "input_text", text: "next" }], internal_chat_message_metadata_passthrough: { content_item_kinds: ["user.text"] } } };
  await writeFile(path.join(evidence, "rollout.jsonl"), `${JSON.stringify(environment)}\n${JSON.stringify(user)}\n`);
  const grade = await gradeAttempt({ attemptRoot: root, scenario: { id: "surface.tui.test", conversation: { initialMessage: "next", turns: [] } }, attempt: { scenarioId: "surface.tui.test", repetition: 1, arm: "base", exitReason: "processExited", process: { code: 0 }, workspace: { path: "C:/workspace", baseline: "baseline" } } });
  assert.equal(grade.validity.gate.infrastructureValid, true);
  assert.equal(grade.validity.gate.messageBoundariesConfirmed, true);
});
