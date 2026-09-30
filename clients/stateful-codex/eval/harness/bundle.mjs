import { mkdir, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { runAttempt } from "./attempt-runner.mjs";
import { planScenarios } from "./planner.mjs";
import { buildScorecard, writeScorecard } from "./scorecard.mjs";

export async function runBundle({ scenarios, codex, out, jobs, reps, tier = "surface", arms = ["base", "stateful"], authHome = process.env.CODEX_HOME ?? path.join(os.homedir(), ".codex"), python, workRoot }) {
  out = path.resolve(out);
  const plan = planScenarios(scenarios, { tier, reps, jobs, arms });
  workRoot = path.resolve(workRoot ?? path.join(os.tmpdir(), "scbench-workspaces", path.basename(out)));
  await mkdir(out, { recursive: true });
  await writeJson(path.join(out, "plan.json"), plan);
  const manifest = { schemaVersion: 1, startedAt: new Date().toISOString(), codex, workRoot, arms, plan, attempts: [] };
  await writeJson(path.join(out, "manifest.json"), manifest);
  const work = [];
  for (let rep = 1; rep <= plan.reps; rep += 1) {
    for (const scenario of scenarios) for (const arm of arms.filter((candidate) => scenario.arms.includes(candidate))) work.push({ scenario, rep, arm });
  }
  const results = await parallel(work, plan.jobs, (item) => runAttempt({ ...item, out, codex, authHome, python, workRoot }));
  manifest.attempts = results.map(({ attempt }) => attempt);
  manifest.finishedAt = new Date().toISOString();
  await writeJson(path.join(out, "manifest.json"), manifest);
  const scorecard = buildScorecard({ manifest, plan, grades: results.map(({ grade }) => grade) });
  await writeScorecard(out, scorecard);
  return scorecard;
}

async function parallel(items, limit, task) {
  const results = [];
  let next = 0;
  async function worker() {
    while (next < items.length) results.push(await task(items[next++]));
  }
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, worker));
  return results;
}

async function writeJson(file, value) {
  await writeFile(file, `${JSON.stringify(value, null, 2)}\n`);
}
