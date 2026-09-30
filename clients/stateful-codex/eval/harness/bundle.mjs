import { createHash } from "node:crypto";
import { access, mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { runTui } from "../adapters/tui.mjs";
import { isolatedEnvironment, prepareIsolatedHome } from "./isolated-home.mjs";
import { planScenarios } from "./planner.mjs";
import { gradeAttempt } from "../graders/tui-evidence.mjs";
import { buildScorecard, writeScorecard } from "./scorecard.mjs";
import { captureWorkspaceDiff, listFiles, prepareWorkspace } from "./workspace.mjs";

export async function runBundle({ scenarios, codex, out, jobs, reps, tier = "surface", authHome = process.env.CODEX_HOME ?? path.join(os.homedir(), ".codex"), python, workRoot }) {
  out = path.resolve(out);
  const plan = planScenarios(scenarios, { tier, reps, jobs });
  workRoot = path.resolve(workRoot ?? path.join(os.tmpdir(), "scbench-workspaces", path.basename(out)));
  await mkdir(out, { recursive: true });
  await writeJson(path.join(out, "plan.json"), plan);
  const manifest = { schemaVersion: 1, startedAt: new Date().toISOString(), codex, workRoot, plan, attempts: [] };
  await writeJson(path.join(out, "manifest.json"), manifest);
  const work = [];
  for (let rep = 1; rep <= reps; rep += 1) for (const scenario of scenarios) work.push({ scenario, rep });
  const results = await parallel(work, plan.jobs, async ({ scenario, rep }) => runAttempt({ scenario, rep, out, codex, authHome, python, workRoot, arm: "stateful" }));
  manifest.attempts = results.map(({ attempt, grade }) => attempt);
  manifest.finishedAt = new Date().toISOString();
  await writeJson(path.join(out, "manifest.json"), manifest);
  const scorecard = buildScorecard({ manifest, plan, grades: results.map(({ grade }) => grade) });
  await writeScorecard(out, scorecard);
  return scorecard;
}

export async function runAttempt({ scenario, rep, out, codex, authHome, python, workRoot, arm, adapter = runTui }) {
  const attemptRoot = path.join(out, "attempts", scenario.id, String(rep));
  const evidence = path.join(attemptRoot, "evidence");
  await mkdir(evidence, { recursive: true });
  const prepared = await prepareWorkspace({ fixtureRoot: scenario.fixture.resolvedSource, workspaceRoot: path.join(workRoot, scenario.id, String(rep), arm) });
  const workspace = prepared.workspace;
  const homes = await prepareIsolatedHome(attemptRoot, authHome);
  const before = await inventory(workspace);
  await writeJson(path.join(evidence, "workspace-before.json"), before);
  const startedAt = new Date().toISOString();
  const adapterResult = await adapter({ attemptRoot, workspace, scenario, codex, python, arm, env: isolatedEnvironment(homes) });
  for (const file of ["transcript.txt", "actions.jsonl", "approvals.jsonl", "turns.json", "screen-final.txt"]) await ensureFile(path.join(evidence, file));
  const after = await listFiles(prepared.workspace);
  await writeJson(path.join(evidence, "workspace-after.json"), after);
  const workspaceChange = await captureWorkspaceDiff(prepared.workspace, path.join(evidence, "workspace.patch"));
  await captureStores(evidence, homes);
  const attempt = { schemaVersion: 1, scenarioId: scenario.id, repetition: rep, arm, fixtureSha256: scenario.fixture.sha256, executableSha256: await fileHash(codex), startedAt, finishedAt: new Date().toISOString(), workspace: { path: prepared.workspace, baseline: prepared.baseline, before: before.length, after: after.length, status: workspaceChange.status }, process: adapterResult, exitReason: adapterResult.code === 0 ? "processExited" : "driverFailed", retryClassification: "none", evidence: await inventory(attemptRoot) };
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
