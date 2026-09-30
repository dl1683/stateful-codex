import { createHash } from "node:crypto";
import { access, mkdir, readdir, readFile, rename, unlink, writeFile } from "node:fs/promises";
import { DatabaseSync } from "node:sqlite";
import os from "node:os";
import path from "node:path";
import { runTui } from "../adapters/tui.mjs";
import { gradeAttempt } from "../graders/tui-evidence.mjs";
import { writeProjectStateArtifact } from "../export-project-state.mjs";
import { isolatedEnvironment, prepareIsolatedHome } from "./isolated-home.mjs";
import { captureWorkspaceDiff, listFiles, prepareWorkspace } from "./workspace.mjs";

export async function runAttempt({ scenario, rep, attemptDirectory = String(rep), arm = "stateful", out, workRoot = path.join(os.tmpdir(), "scbench-test-workspaces"), codex, authHome, python, adapter = runTui }) {
  const attemptRoot = path.join(out, "attempts", scenario.id, arm, attemptDirectory);
  await mkdir(path.dirname(attemptRoot), { recursive: true });
  await mkdir(attemptRoot);
  const evidence = path.join(attemptRoot, "evidence");
  await mkdir(evidence);
  let prepared;
  let homes;
  try {
    prepared = await prepareWorkspace({ fixtureRoot: scenario.fixture.resolvedSource, workspaceRoot: path.join(workRoot, scenario.id, arm, attemptDirectory) });
    homes = await prepareIsolatedHome(attemptRoot, authHome);
  } catch (error) {
    return sealFailure({ attemptRoot, evidence, scenario, rep, arm, codex, error, reason: "setupFailed" });
  }
  const sandbox = await sandboxPreflight(prepared.workspace, homes.codexHome);
  await writeJson(path.join(evidence, "sandbox-preflight.json"), sandbox);
  const before = await listFiles(prepared.workspace);
  await writeJson(path.join(evidence, "workspace-before.json"), before);
  const startedAt = new Date().toISOString();
  let adapterResult;
  let error = null;
  try {
    if (!sandbox.writable || !sandbox.seeded) adapterResult = { code: null, signal: null, exitReason: "sandboxPreflightFailed", error: sandbox.error };
    else adapterResult = await adapter({ attemptRoot, workspace: prepared.workspace, scenario, codex, python, arm, env: isolatedEnvironment(homes), timeoutSeconds: scenario.conversation.turnTimeoutSeconds * (scenario.conversation.turns.length + 1) });
  } catch (caught) {
    error = caught;
    adapterResult = { code: null, signal: null, exitReason: "runnerError", error: caught.message };
  }
  const after = await listFiles(prepared.workspace);
  await writeJson(path.join(evidence, "workspace-after.json"), after);
  const workspaceChange = await captureWorkspaceDiff(prepared.workspace, path.join(evidence, "workspace.patch"));
  const capture = await captureStores({ evidence, homes, arm, startedAt, scenario });
  const attempt = {
    schemaVersion: 1,
    scenarioId: scenario.id,
    repetition: rep,
    arm,
    fixtureSha256: scenario.fixture.sha256,
    executableSha256: await fileHash(codex),
    startedAt,
    finishedAt: new Date().toISOString(),
    workspace: { path: prepared.workspace, baseline: prepared.baseline, before: before.length, after: after.length, status: workspaceChange.status },
    process: adapterResult,
    exitReason: adapterResult.exitReason ?? (adapterResult.code === 0 ? "processExited" : "driverFailed"),
    retryClassification: classifyRetry(adapterResult, error),
    capture,
    evidence: await inventory(attemptRoot),
  };
  await atomicJson(path.join(attemptRoot, "attempt.json"), attempt);
  const grade = await gradeAttempt({ attemptRoot, scenario, attempt });
  await writeJson(path.join(attemptRoot, "grade.json"), grade);
  return { attempt, grade };
}

async function sealFailure({ attemptRoot, evidence, scenario, rep, arm, codex, error, reason }) {
  const attempt = {
    schemaVersion: 1,
    scenarioId: scenario.id,
    repetition: rep,
    arm,
    fixtureSha256: scenario.fixture.sha256,
    executableSha256: await safeFileHash(codex),
    startedAt: new Date().toISOString(),
    finishedAt: new Date().toISOString(),
    workspace: null,
    process: { code: null, signal: null, exitReason: reason, error: error.message },
    exitReason: reason,
    retryClassification: "none",
    evidence: await inventory(attemptRoot),
  };
  await atomicJson(path.join(attemptRoot, "attempt.json"), attempt);
  const grade = await gradeAttempt({ attemptRoot, scenario, attempt });
  await writeJson(path.join(attemptRoot, "grade.json"), grade);
  return { attempt, grade };
}

async function sandboxPreflight(workspace, codexHome) {
  const canary = path.join(workspace, `.scbench-write-canary-${process.pid}`);
  const seeded = (await Promise.all([".sandbox", ".sandbox-bin", "cap_sid"].map(async (name) => {
    try { await access(path.join(codexHome, name)); return true; } catch { return false; }
  }))).every(Boolean);
  try {
    await writeFile(canary, "scbench preflight\n");
    await unlink(canary);
    return { writable: true, seeded, canary: "write-read-delete", error: seeded ? null : "sandbox runtime seed is incomplete" };
  } catch (error) {
    return { writable: false, seeded, canary: "write-read-delete", error: error.message };
  }
}

async function captureStores({ evidence, homes, arm, startedAt, scenario }) {
  const files = await inventory(homes.sqliteHome);
  await writeJson(path.join(evidence, "store-inventory.json"), { root: homes.sqliteHome, files, capturedAt: new Date().toISOString() });
  const snapshot = await snapshotSqlite(homes.sqliteHome, path.join(evidence, "store-snapshot"));
  await writeJson(path.join(evidence, "store-snapshot.json"), snapshot);
  const rollout = await selectRollout(homes.codexHome, startedAt);
  if (rollout) {
    await writeFile(path.join(evidence, "rollout.jsonl"), rollout.content);
    await writeJson(path.join(evidence, "rollout-source.json"), { path: rollout.path, sha256: hashText(rollout.content), threadId: rollout.threadId, candidates: rollout.candidates });
  }
  if (arm === "stateful" && rollout?.threadId) {
    try {
      const state = await writeProjectStateArtifact({ sqliteHome: homes.sqliteHome, threadId: rollout.threadId, output: path.join(evidence, "project-state.json"), capturedAtMs: Date.now() });
      return { rollout: true, threadId: rollout.threadId, state: true, stateSha256: state.snapshotSha256, storeSnapshot: snapshot };
    } catch (error) {
      await writeJson(path.join(evidence, "project-state.json"), { available: false, applicable: true, reason: error.message, source: "export-project-state" });
    }
  } else {
    await writeJson(path.join(evidence, "project-state.json"), { available: false, applicable: arm === "stateful", reason: arm === "base" ? "base arm has no Stateful project state" : "rollout thread was not identified" });
  }
  return { rollout: Boolean(rollout), threadId: rollout?.threadId ?? null, state: false, storeSnapshot: snapshot };
}

async function selectRollout(codexHome, startedAt) {
  const files = await findFiles(path.join(codexHome, "sessions"), (name) => name.endsWith(".jsonl"));
  const candidates = [];
  for (const file of files) {
    const content = await readFile(file, "utf8");
    if (!content.trim()) continue;
    const threadId = extractThreadId(content);
    candidates.push({ path: file, content, threadId, modified: (await import("node:fs/promises")).stat(file).then((value) => value.mtimeMs) });
  }
  if (!candidates.length) return null;
  const resolved = await Promise.all(candidates.map(async (candidate) => ({ ...candidate, modified: await candidate.modified })));
  resolved.sort((left, right) => right.modified - left.modified);
  const selected = resolved[0];
  return { ...selected, candidates: resolved.map(({ path: candidatePath, threadId }) => ({ path: candidatePath, threadId })) };
}

function extractThreadId(content) {
  for (const line of content.split(/\r?\n/)) {
    try {
      const event = JSON.parse(line);
      if (event.type === "session_meta") return event.payload?.id ?? event.payload?.session_id ?? null;
      if (event.thread_id) return event.thread_id;
    } catch {
      // Ignore non-JSON rollout lines.
    }
  }
  return null;
}

async function snapshotSqlite(root, outputRoot) {
  await mkdir(outputRoot, { recursive: true });
  const databases = (await inventory(root)).filter((file) => file.endsWith(".sqlite"));
  const snapshots = [];
  for (const file of databases) {
    const source = path.join(root, file);
    const destination = path.join(outputRoot, file);
    try {
      const database = new DatabaseSync(source, { readOnly: true });
      database.exec(`VACUUM INTO '${destination.replaceAll("'", "''")}'`);
      database.close();
      snapshots.push({ file, path: destination, method: "vacuum-into", sha256: await fileHash(destination) });
    } catch (error) {
      snapshots.push({ file, error: error.message });
    }
  }
  return { formatVersion: 1, consistent: snapshots.every(({ error }) => !error), databases: snapshots };
}

function classifyRetry(result, error) {
  const text = `${error?.message ?? ""} ${result?.error ?? ""}`;
  return result?.exitReason === "spawnError" || /capacity|transport|connection reset|429/i.test(text) ? "infrastructure-before-agent" : "none";
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
  try {
    return (await inventory(root)).filter(predicate).map((file) => path.join(root, file));
  } catch {
    return [];
  }
}

async function fileHash(file) { return `sha256:${createHash("sha256").update(await readFile(file)).digest("hex")}`; }
async function safeFileHash(file) { try { return await fileHash(file); } catch { return null; } }
function hashText(value) { return createHash("sha256").update(value).digest("hex"); }
async function writeJson(file, value) { await writeFile(file, `${JSON.stringify(value, null, 2)}\n`); }
async function atomicJson(file, value) { const temporary = `${file}.tmp-${process.pid}`; await writeJson(temporary, value); await rename(temporary, file); }
