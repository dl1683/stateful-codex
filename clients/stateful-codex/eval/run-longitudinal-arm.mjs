import { spawn } from "node:child_process";
import { createWriteStream } from "node:fs";
import { link, mkdir, readFile, rename, stat, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import path from "node:path";
import { finished } from "node:stream/promises";
import { pathToFileURL } from "node:url";

import { corpusHash } from "./corpus-hash.mjs";
import { writeProjectStateArtifact } from "./export-project-state.mjs";
import { findRolloutPath } from "./rollout-path.mjs";
import {
  applyScheduledIntervention,
  validateInterventionSchedule,
} from "./source-intervention.mjs";

const MEMORY_ISOLATION_ARGS = [
  "-c",
  "memories.use_memories=false",
  "-c",
  "memories.generate_memories=false",
];

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const manifest = JSON.parse(await readFile(options.manifest, "utf8"));
  if (manifest.memoryPolicy !== "disabled") {
    throw new Error("longitudinal manifests must explicitly disable host memory");
  }
  const project = manifest.projects.find((candidate) => candidate.id === options.project);
  if (!project) throw new Error(`unknown project: ${options.project}`);
  const snapshot = JSON.parse(
    await readFile(path.join(options.snapshotRoot, project.id, "snapshot.json"), "utf8"),
  );
  const workspace = snapshot[options.arm];
  const scheduledInterventions = await validateInterventionSchedule(
    project.cases,
    `sha256:${snapshot.corpus.sha256}`,
    path.dirname(options.manifest),
  );
  if (
    JSON.stringify(scheduledInterventions) !==
    JSON.stringify(snapshot.interventionSchedule ?? [])
  ) {
    throw new Error(`source intervention schedule changed for ${project.id}`);
  }
  const resultRoot = path.join(options.output, project.id, options.arm);
  await mkdir(resultRoot, { recursive: true });
  await prepareIsolatedCodexHome(options.codexHome, options.authHome);
  const statePath = path.join(resultRoot, "run-state.json");
  const initialRevision = `sha256:${snapshot.corpus.sha256}`;
  const state = await readState(
    statePath,
    project.id,
    options.arm,
    initialRevision,
  );
  if (options.reconcileFailed) {
    const benchmarkCase = project.cases[state.nextCase];
    if (!benchmarkCase) {
      throw new Error("there is no pending case with a failed attempt to reconcile");
    }
    reconcileFailedAttempt(state, benchmarkCase.id, options.reconcileFailed);
    await writeState(statePath, state);
  }
  const openingHash = await corpusHash(workspace);
  if (`sha256:${openingHash.sha256}` !== state.corpusRevision) {
    throw new Error(`corpus changed before ${project.id}/${options.arm}`);
  }
  await assertChatGptLogin(
    options.codex,
    workspace,
    options.sqliteHome,
    options.codexHome,
  );

  for (let index = state.nextCase; index < project.cases.length; index += 1) {
    const benchmarkCase = project.cases[index];
    const intervention = await applyScheduledIntervention({
      workspace,
      manifestDirectory: path.dirname(options.manifest),
      intervention: benchmarkCase.intervention,
      state,
    });
    if (intervention.applied) await writeState(statePath, state);
    const unresolved = unresolvedAttemptForCase(state, benchmarkCase.id);
    if (unresolved) {
      throw new Error(
        `attempt ${unresolved.number} for ${benchmarkCase.id} is ${unresolved.status}; reconcile its canonical turn before retrying`,
      );
    }
    const attempt = {
      caseId: benchmarkCase.id,
      number: nextAttemptNumber(state, benchmarkCase.id),
      status: "running",
      startedAtMs: Date.now(),
      completedAtMs: null,
      durationMs: null,
      exitCode: null,
      threadId: state.threadId,
      error: null,
    };
    state.attempts.push(attempt);
    await writeState(statePath, state);
    console.error(`[${project.id}/${options.arm}] ${benchmarkCase.id}`);
    let output;
    try {
      output = await runTurn({
        ...options,
        workspace,
        resultRoot,
        index,
        attempt: attempt.number,
        prompt: benchmarkCase.prompt,
        threadId: state.threadId,
        mode: manifest.mode ?? "autonomous",
        model: manifest.model,
        reasoningEffort: manifest.reasoningEffort ?? "high",
        compaction: manifest.compaction ?? null,
      });
    } catch (error) {
      finishAttempt(attempt, { status: "failed", error });
      await writeState(statePath, state);
      throw error;
    }
    state.threadId ??= output.threadId;
    attempt.threadId = state.threadId;
    if (!state.threadId) {
      const error = new Error("Codex output did not report a thread ID");
      finishAttempt(attempt, { status: "failed", error, exitCode: output.exitCode });
      await writeState(statePath, state);
      throw error;
    }
    if (!state.rolloutPath) {
      try {
        state.rolloutPath = await findRolloutPath(
          path.join(options.codexHome, "sessions"),
          state.threadId,
        );
      } catch (error) {
        finishAttempt(attempt, { status: "invalid", error, exitCode: output.exitCode });
        await writeState(statePath, state);
        throw error;
      }
    }
    const currentHash = await corpusHash(workspace);
    if (`sha256:${currentHash.sha256}` !== state.corpusRevision) {
      const error = new Error(
        `corpus changed during ${project.id}/${options.arm}/${benchmarkCase.id}`,
      );
      finishAttempt(attempt, { status: "invalid", error, exitCode: output.exitCode });
      await writeState(statePath, state);
      throw error;
    }
    const turnRecord = {
      id: benchmarkCase.id,
      attempt: attempt.number,
      exitCode: output.exitCode,
      corpusRevision: state.corpusRevision,
      intervention: intervention.record,
    };
    if (output.exitCode !== 0) {
      const error = new Error(`Codex exited ${output.exitCode}`);
      finishAttempt(attempt, { status: "failed", error, exitCode: output.exitCode });
      await writeState(statePath, state);
      throw error;
    }
    if (options.arm === "stateful") {
      const stateArtifactPath = path.join(
        resultRoot,
        `turn-${String(index + 1).padStart(2, "0")}-state.json`,
      );
      let stateArtifact;
      try {
        stateArtifact = await writeProjectStateArtifact({
          sqliteHome: options.sqliteHome,
          threadId: state.threadId,
          corpusRevision: turnRecord.corpusRevision,
          output: stateArtifactPath,
        });
      } catch (error) {
        finishAttempt(attempt, { status: "invalid", error, exitCode: output.exitCode });
        await writeState(statePath, state);
        throw error;
      }
      turnRecord.stateArtifact = {
        path: stateArtifactPath,
        snapshotSha256: stateArtifact.snapshotSha256,
        capturedAtMs: stateArtifact.capturedAtMs,
        intelligenceRevision: stateArtifact.intelligenceRevision,
        runRevision: stateArtifact.run.revision,
      };
    }
    finishAttempt(attempt, { status: "completed", exitCode: output.exitCode });
    state.turns.push(turnRecord);
    state.nextCase = index + 1;
    await writeState(statePath, state);
  }
}

async function runTurn(options) {
  const turn = String(options.index + 1).padStart(2, "0");
  const attempt = String(options.attempt).padStart(2, "0");
  const args = options.threadId
    ? resumeArgs(options)
    : startArgs(options);
  const stdoutFile = createWriteStream(
    path.join(options.resultRoot, `turn-${turn}-attempt-${attempt}.stdout.jsonl`),
  );
  const stderrFile = createWriteStream(
    path.join(options.resultRoot, `turn-${turn}-attempt-${attempt}.stderr.txt`),
  );
  const child = spawn(options.codex, args, {
    cwd: options.workspace,
    env: authEnvironment(options.sqliteHome, options.codexHome),
    windowsHide: true,
  });
  child.stdin.end();
  child.stdout.pipe(stdoutFile);
  child.stderr.pipe(stderrFile);
  let threadId = options.threadId;
  let pendingLine = "";
  child.stdout.on("data", (chunk) => {
    if (threadId) return;
    const lines = `${pendingLine}${chunk}`.split(/\r?\n/);
    pendingLine = lines.pop() ?? "";
    for (const line of lines) {
      threadId ??= extractThreadId(line);
    }
    if (pendingLine.length > 65_536) pendingLine = "";
  });
  const exitCode = await new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", resolve);
  });
  await Promise.all([finished(stdoutFile), finished(stderrFile)]);
  threadId ??= extractThreadId(pendingLine);
  return { exitCode, threadId };
}

function startArgs(options) {
  const args = [
    "exec",
    "--json",
    "--skip-git-repo-check",
    "--model",
    options.model,
    "-c",
    `model_reasoning_effort=\"${options.reasoningEffort}\"`,
    ...compactionArgs(options.compaction),
    ...MEMORY_ISOLATION_ARGS,
    ...platformSandboxArgs(),
    "--sandbox",
    "read-only",
    "-C",
    options.workspace,
  ];
  if (options.arm === "stateful") args.push("--stateful", options.mode);
  args.push(options.prompt);
  return args;
}

export function resumeArgs(options) {
  const args = [
    "exec",
    "resume",
    "--json",
    "--skip-git-repo-check",
    "--model",
    options.model,
    "-c",
    `model_reasoning_effort=\"${options.reasoningEffort}\"`,
    ...compactionArgs(options.compaction),
    ...MEMORY_ISOLATION_ARGS,
    ...platformSandboxArgs(),
    "-c",
    "sandbox_mode=\"read-only\"",
  ];
  if (options.arm === "stateful") args.push("--stateful", options.mode);
  args.push(options.threadId, options.prompt);
  return args;
}

export function compactionArgs(compaction) {
  if (!compaction) return [];
  if (
    !Number.isSafeInteger(compaction.autoCompactTokenLimit) ||
    compaction.autoCompactTokenLimit < 1_000
  ) {
    throw new Error("compaction.autoCompactTokenLimit must be an integer of at least 1000");
  }
  const scope = compaction.scope ?? "total";
  if (!["total", "body_after_prefix"].includes(scope)) {
    throw new Error("compaction.scope must be total or body_after_prefix");
  }
  return [
    "-c",
    `model_auto_compact_token_limit=${compaction.autoCompactTokenLimit}`,
    "-c",
    `model_auto_compact_token_limit_scope="${scope}"`,
  ];
}

export function platformSandboxArgs(platform = process.platform) {
  return platform === "win32" ? ["-c", 'windows.sandbox="unelevated"'] : [];
}

function extractThreadId(output) {
  for (const line of output.split(/\r?\n/)) {
    try {
      const event = JSON.parse(line);
      if (event.type === "thread.started") return event.thread_id;
      if (event.thread_id && event.type?.includes("thread")) return event.thread_id;
    } catch {
      // Ignore non-JSON status lines.
    }
  }
  return null;
}

async function assertChatGptLogin(codex, cwd, sqliteHome, codexHome) {
  const result = await capture(
    codex,
    ["login", "status"],
    cwd,
    sqliteHome,
    codexHome,
  );
  if (
    result.exitCode !== 0 ||
    !`${result.stdout}\n${result.stderr}`.includes("Logged in using ChatGPT")
  ) {
    throw new Error("cached Codex ChatGPT login is required");
  }
}

async function capture(command, args, cwd, sqliteHome, codexHome) {
  const child = spawn(command, args, {
    cwd,
    env: authEnvironment(sqliteHome, codexHome),
    windowsHide: true,
  });
  child.stdin.end();
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (chunk) => (stdout += chunk));
  child.stderr.on("data", (chunk) => (stderr += chunk));
  const exitCode = await new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", resolve);
  });
  return { exitCode, stdout, stderr };
}

export async function prepareIsolatedCodexHome(codexHome, authHome) {
  await mkdir(codexHome, { recursive: true });
  const source = path.join(authHome, "auth.json");
  const target = path.join(codexHome, "auth.json");
  if (path.resolve(source) === path.resolve(target)) return;
  try {
    await link(source, target);
  } catch (error) {
    if (error.code !== "EEXIST") {
      throw new Error(
        `could not link cached ChatGPT credentials into isolated Codex home: ${error.message}`,
      );
    }
    const [sourceStat, targetStat] = await Promise.all([stat(source), stat(target)]);
    if (sourceStat.dev !== targetStat.dev || sourceStat.ino !== targetStat.ino) {
      throw new Error(
        "isolated Codex home contains an auth.json that is not linked to the configured auth home",
      );
    }
  }
}

export function authEnvironment(sqliteHome, codexHome) {
  const environment = { ...process.env };
  delete environment.OPENAI_API_KEY;
  delete environment.CODEX_API_KEY;
  environment.CODEX_SQLITE_HOME = sqliteHome;
  environment.CODEX_HOME = codexHome;
  return environment;
}

async function readState(statePath, project, arm, initialRevision) {
  try {
    const state = JSON.parse(await readFile(statePath, "utf8"));
    state.attempts ??= [];
    state.appliedInterventions ??= [];
    state.corpusRevision ??= state.turns.at(-1)?.corpusRevision ?? initialRevision;
    return state;
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
    return {
      project,
      arm,
      threadId: null,
      rolloutPath: null,
      nextCase: 0,
      corpusRevision: initialRevision,
      appliedInterventions: [],
      turns: [],
      attempts: [],
    };
  }
}

export function nextAttemptNumber(state, caseId) {
  return state.attempts.filter((attempt) => attempt.caseId === caseId).length + 1;
}

export function unresolvedAttemptForCase(state, caseId) {
  return state.attempts.find(
    (attempt) =>
      attempt.caseId === caseId &&
      !["completed", "reconciled"].includes(attempt.status),
  );
}

export function reconcileFailedAttempt(state, caseId, reason) {
  const attempt = unresolvedAttemptForCase(state, caseId);
  if (!attempt) throw new Error(`no unresolved attempt for ${caseId}`);
  if (attempt.status !== "failed") {
    throw new Error(
      `attempt ${attempt.number} for ${caseId} is ${attempt.status}, not failed`,
    );
  }
  attempt.status = "reconciled";
  attempt.reconciledAtMs = Date.now();
  attempt.reconciliation = reason;
  return attempt;
}

function finishAttempt(attempt, { status, exitCode = null, error = null }) {
  attempt.status = status;
  attempt.completedAtMs = Date.now();
  attempt.durationMs = attempt.completedAtMs - attempt.startedAtMs;
  attempt.exitCode = exitCode;
  attempt.error = error == null ? null : String(error.message ?? error);
}

async function writeState(statePath, state) {
  const temporaryPath = `${statePath}.tmp-${process.pid}`;
  await writeFile(temporaryPath, `${JSON.stringify(state, null, 2)}\n`);
  await rename(temporaryPath, statePath);
}

function parseArgs(args) {
  const options = {};
  for (let index = 0; index < args.length; index += 2) {
    const [argument, value] = args.slice(index, index + 2);
    if (argument === "--manifest") options.manifest = value;
    else if (argument === "--project") options.project = value;
    else if (argument === "--arm") options.arm = value;
    else if (argument === "--snapshot-root") options.snapshotRoot = value;
    else if (argument === "--output") options.output = value;
    else if (argument === "--codex") options.codex = value;
    else if (argument === "--sqlite-home") options.sqliteHome = value;
    else if (argument === "--codex-home") options.codexHome = value;
    else if (argument === "--auth-home") options.authHome = value;
    else if (argument === "--reconcile-failed") options.reconcileFailed = value;
    else throw new Error(`unknown argument: ${argument}`);
  }
  if (
    !options.manifest ||
    !options.project ||
    !["baseline", "stateful"].includes(options.arm) ||
    !options.snapshotRoot ||
    !options.output ||
    !options.codex
  ) {
    throw new Error(
      "usage: --manifest PATH --project ID --arm baseline|stateful --snapshot-root PATH --output PATH --codex PATH [--sqlite-home PATH] [--codex-home PATH] [--auth-home PATH] [--reconcile-failed REASON]",
    );
  }
  for (const field of ["manifest", "snapshotRoot", "output", "codex"]) {
    options[field] = path.resolve(options[field]);
  }
  options.sqliteHome = path.resolve(
    options.sqliteHome ??
      process.env.CODEX_SQLITE_HOME ??
      process.env.CODEX_HOME ??
      path.join(homedir(), ".codex"),
  );
  options.authHome = path.resolve(
    options.authHome ?? process.env.CODEX_HOME ?? path.join(homedir(), ".codex"),
  );
  options.codexHome = path.resolve(
    options.codexHome ?? path.join(options.sqliteHome, "eval-codex-home"),
  );
  return options;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
