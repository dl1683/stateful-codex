import { createHash } from "node:crypto";
import { access, cp, mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import os from "node:os";
import path from "node:path";
import { runTui } from "../adapters/tui.mjs";
import { isolatedEnvironment, prepareIsolatedHome } from "./isolated-home.mjs";
import { planScenarios } from "./planner.mjs";
import { gradeAttempt } from "../graders/tui-evidence.mjs";
import { buildScorecard, writeScorecard } from "./scorecard.mjs";

export async function runBundle({ scenarios, codex, out, jobs, reps = 1, tier = "surface", authHome = process.env.CODEX_HOME ?? path.join(os.homedir(), ".codex"), python }) {
  out = path.resolve(out);
  const plan = planScenarios(scenarios, { tier, reps, jobs });
  await mkdir(out, { recursive: true });
  await writeJson(path.join(out, "plan.json"), plan);
  const manifest = { schemaVersion: 1, startedAt: new Date().toISOString(), codex, plan, attempts: [] };
  await writeJson(path.join(out, "manifest.json"), manifest);
  const work = [];
  for (let rep = 1; rep <= reps; rep += 1) for (const scenario of scenarios) work.push({ scenario, rep });
  const results = await parallel(work, plan.jobs, async ({ scenario, rep }) => runAttempt({ scenario, rep, out, codex, authHome, python }));
  manifest.attempts = results.map(({ attempt, grade }) => attempt);
  manifest.finishedAt = new Date().toISOString();
  await writeJson(path.join(out, "manifest.json"), manifest);
  const scorecard = buildScorecard({ manifest, plan, grades: results.map(({ grade }) => grade) });
  await writeScorecard(out, scorecard);
  return scorecard;
}

export async function runAttempt({ scenario, rep, out, codex, authHome, python, adapter = runTui }) {
  const attemptRoot = path.join(out, "attempts", scenario.id, String(rep));
  const workspace = path.join(attemptRoot, "workspace");
  const evidence = path.join(attemptRoot, "evidence");
  await mkdir(evidence, { recursive: true });
  await cp(scenario.fixture.resolvedSource, workspace, { recursive: true });
  const homes = await prepareIsolatedHome(attemptRoot, authHome);
  const before = await inventory(workspace);
  await writeJson(path.join(evidence, "workspace-before.json"), before);
  const startedAt = new Date().toISOString();
  const adapterResult = await adapter({ attemptRoot, scenario, codex, python, env: isolatedEnvironment(homes) });
  for (const file of ["transcript.txt", "actions.jsonl", "approvals.jsonl", "turns.json", "screen-final.txt"]) await ensureFile(path.join(evidence, file));
  const after = await inventory(workspace);
  await writeJson(path.join(evidence, "workspace-after.json"), after);
  await writeFile(path.join(evidence, "workspace.patch"), spawnSync("git", ["-C", workspace, "diff", "--binary"], { encoding: "utf8" }).stdout ?? "");
  await captureStores(evidence, homes);
  const attempt = { schemaVersion: 1, scenarioId: scenario.id, repetition: rep, fixtureSha256: scenario.fixture.sha256, executableSha256: await fileHash(codex), startedAt, finishedAt: new Date().toISOString(), process: adapterResult, exitReason: adapterResult.code === 0 ? "processExited" : "driverFailed", retryClassification: "none", evidence: await inventory(attemptRoot) };
  await writeJson(path.join(attemptRoot, "attempt.json"), attempt);
  const grade = await gradeAttempt({ attemptRoot, scenario, attempt });
  await writeJson(path.join(attemptRoot, "grade.json"), grade);
  return { attempt, grade };
}

async function captureStores(evidence, homes) {
  const files = await inventory(homes.sqliteHome);
  await writeJson(path.join(evidence, "store-inventory.json"), { root: homes.sqliteHome, files });
  const rollouts = (await findFiles(homes.codexHome, (name) => name.endsWith(".jsonl"))).map((file) => readFile(file, "utf8"));
  await writeFile(path.join(evidence, "rollout.jsonl"), (await Promise.all(rollouts)).join("\n"));
  await writeJson(path.join(evidence, "project-state.json"), { available: false, source: "isolated SQLite home", storeFiles: files });
}

async function parallel(items, limit, task) {
  const results = []; let next = 0;
  async function worker() { while (next < items.length) { const item = items[next++]; results.push(await task(item)); } }
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, worker));
  return results;
}

async function inventory(root) {
  const files = [];
  for (const entry of await readdir(root, { withFileTypes: true })) {
    const full = path.join(root, entry.name);
    if (entry.isDirectory()) files.push(...(await inventory(full)).map((name) => path.join(entry.name, name)));
    else files.push(entry.name);
  }
  return files.map((file) => file.replaceAll(path.sep, "/")).sort();
}

async function findFiles(root, predicate) {
  const files = [];
  for (const entry of await readdir(root, { withFileTypes: true })) {
    const full = path.join(root, entry.name);
    if (entry.isDirectory()) files.push(...await findFiles(full, predicate));
    else if (predicate(entry.name)) files.push(full);
  }
  return files;
}

async function fileHash(file) { return `sha256:${createHash("sha256").update(await readFile(file)).digest("hex")}`; }
async function ensureFile(file) { try { await access(file); } catch { await writeFile(file, ""); } }
async function writeJson(file, value) { await writeFile(file, `${JSON.stringify(value, null, 2)}\n`); }
