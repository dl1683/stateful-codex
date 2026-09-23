import { spawn } from "node:child_process";
import {
  access,
  cp,
  mkdir,
  readFile,
  readdir,
  rename,
  writeFile,
} from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

const POLL_INTERVAL_MS = 10_000;
const MAX_FEEDBACK_CHARS = 7_000;
const MAX_INFRA_RETRIES = 3;

async function main() {
  const options = parseArgs(process.argv.slice(2));
  await validateOptions(options);

  const cohort = JSON.parse(await readFile(options.cohort, "utf8"));
  const tasks = await prioritizeStartedTasks(cohort, options);
  const semaphore = new Semaphore(options.concurrency);

  const results = await Promise.allSettled(
    tasks.map((entry) => runTaskSeries(entry, options, semaphore)),
  );
  const failures = results.filter((result) => result.status === "rejected");
  if (failures.length > 0) {
    for (const failure of failures) console.error(failure.reason);
    throw new Error(`${failures.length} task series failed`);
  }
}

async function runTaskSeries(entry, options, semaphore) {
  let previousResult = await findColdResult(entry, options.jobsDir);

  for (let attempt = 2; attempt <= options.attempts; attempt += 1) {
    const completedJob = await readJsonIfPresent(
      path.join(
        options.jobsDir,
        `${options.jobPrefix}-${entry.task}-a${attempt}`,
        "result.json",
      ),
    );
    if (completedJob?.finished_at && completedJob.stats?.n_errored_trials === 0) {
      const outcome = await runValidAttempt({
        entry,
        attempt,
        previousResult,
        options,
      });
      previousResult = outcome.resultPath;
      await recordControllerOutcome(options, entry.task, attempt, outcome);
      continue;
    }

    const release = await semaphore.acquire();
    try {
      const outcome = await runValidAttempt({
        entry,
        attempt,
        previousResult,
        options,
      });
      previousResult = outcome.resultPath;
      await recordControllerOutcome(options, entry.task, attempt, outcome);
    } finally {
      release();
    }
  }
}

async function runValidAttempt({ entry, attempt, previousResult, options }) {
  for (let infraRetry = 0; infraRetry <= MAX_INFRA_RETRIES; infraRetry += 1) {
    const suffix = infraRetry === 0 ? "" : `-infra-r${infraRetry}`;
    const jobName = `${options.jobPrefix}-${entry.task}-a${attempt}${suffix}`;
    const jobDir = path.join(options.jobsDir, jobName);
    const stateDir = path.join(options.stateRoot, entry.task, "feedback-state");
    const feedback = await buildFeedback(previousResult, attempt - 1);

    const existingJobResult = await readJsonIfPresent(
      path.join(jobDir, "result.json"),
    );
    let harborProcess = null;
    if (!existingJobResult) {
      await snapshotAttemptBaseline({
        stateDir,
        stateRoot: options.stateRoot,
        task: entry.task,
        attempt,
      });
      harborProcess = launchHarbor({
        task: entry.task,
        jobName,
        stateDir,
        feedback,
        options,
      });
    }

    await waitForJob(jobDir, options.jobTimeoutMs, harborProcess);
    const trial = await readOnlyTrialResult(jobDir);
    await validateTrialIdentity(trial, entry.task, stateDir);

    if (!isPreAgentInfrastructureFailure(trial.result)) {
      await access(path.join(trial.trialDir, "agent", "stateful-state.sha256"));
      const memoryMutationObserved = await observedBlackboardMutation(
        trial.trialDir,
      );
      if (!memoryMutationObserved) {
        throw new Error(
          `${jobName} completed without a durable blackboard mutation; reconcile the outcome before continuing`,
        );
      }
      return {
        jobName,
        resultPath: trial.resultPath,
        reward: trial.result.verifier_result?.rewards?.reward ?? null,
        exception: trial.result.exception_info?.exception_type ?? null,
        infrastructureRetries: infraRetry,
        memoryMutationObserved,
      };
    }

    console.error(
      `[${entry.task}/attempt-${attempt}] pre-agent infrastructure failure; retry ${infraRetry + 1}/${MAX_INFRA_RETRIES}`,
    );
  }

  throw new Error(
    `${entry.task} attempt ${attempt} exceeded pre-agent infrastructure retries`,
  );
}

function launchHarbor({ task, jobName, stateDir, feedback, options }) {
  const mount = JSON.stringify([
    {
      type: "bind",
      source: stateDir.replaceAll("\\", "/"),
      target: "/tmp/stateful-codex-state",
    },
  ]);
  const instruction = [
    "Feedback-aware longitudinal Stateful study; this is not an official independent Terminal-Bench attempt.",
    feedback,
    "This authoritative outcome is part of the same project's work history.",
    "At the start, reconcile it with existing project intelligence and supersede any contradicted claim.",
    "Reproduce the requested outcome in this fresh workspace without assuming prior workspace artifacts exist.",
    "Before completion, persist materially reusable successful mechanisms, failures or rejected approaches, verifier evidence, and lifecycle or uncertainty boundaries into project memory; do not merely report activity.",
  ].join(" ");
  const args = [
    "run",
    "--dataset",
    options.dataset,
    "--include-task-name",
    `terminal-bench/${task}`,
    "--n-attempts",
    "1",
    "--n-concurrent",
    "1",
    "--max-retries",
    "0",
    "--agent",
    "stateful_harbor.stateful_codex:StatefulCodex",
    "--model",
    options.model,
    "--effort",
    options.effort,
    "--mounts",
    mount,
    "--extra-instruction",
    instruction,
    "--job-name",
    jobName,
    "--jobs-dir",
    options.jobsDir,
    "--yes",
  ];
  const env = { ...process.env };
  env.CODEX_AUTH_JSON_PATH = options.auth;
  env.STATEFUL_CODEX_BUNDLE_PATH = options.bundle;
  env.STATEFUL_CODEX_BUNDLE_SHA256 = options.bundleSha256;
  env.PYTHONPATH = options.pythonPath;
  delete env.OPENAI_API_KEY;
  delete env.AZURE_OPENAI_API_KEY;
  delete env.CODEX_API_KEY;

  console.error(`[${task}] launching ${jobName}`);
  const child = spawn(options.harbor, args, {
    cwd: options.cwd,
    env,
    stdio: ["ignore", "ignore", "inherit"],
    windowsHide: true,
  });
  child.on("error", (error) => {
    console.error(`[${task}] failed to launch ${jobName}: ${error.message}`);
  });
  return child;
}

async function waitForJob(jobDir, timeoutMs, harborProcess) {
  const startedAt = Date.now();
  while (Date.now() - startedAt < timeoutMs) {
    const result = await readJsonIfPresent(path.join(jobDir, "result.json"));
    if (result?.finished_at) return result;
    if (harborProcess?.exitCode != null) {
      throw new Error(
        `Harbor exited with code ${harborProcess.exitCode} before finishing ${jobDir}`,
      );
    }
    await delay(POLL_INTERVAL_MS);
  }
  throw new Error(`timed out waiting for ${jobDir}`);
}

async function snapshotAttemptBaseline({ stateDir, stateRoot, task, attempt }) {
  const baselineRoot = path.join(stateRoot, "feedback-series-baselines");
  const baseline = path.join(baselineRoot, `${task}-a${attempt}`);
  if (await exists(baseline)) return;
  await mkdir(baselineRoot, { recursive: true });
  const temporary = `${baseline}.tmp`;
  await cp(stateDir, temporary, { recursive: true, errorOnExist: true });
  await rename(temporary, baseline);
}

async function readOnlyTrialResult(jobDir) {
  const entries = await readdir(jobDir, { withFileTypes: true });
  const candidates = [];
  for (const entry of entries) {
    if (!entry.isDirectory()) continue;
    const resultPath = path.join(jobDir, entry.name, "result.json");
    const result = await readJsonIfPresent(resultPath);
    if (result) candidates.push({ trialDir: path.dirname(resultPath), resultPath, result });
  }
  if (candidates.length !== 1) {
    throw new Error(`${jobDir} contains ${candidates.length} trial results; expected one`);
  }
  return candidates[0];
}

async function validateTrialIdentity(trial, task, stateDir) {
  if (trial.result.task_name !== `terminal-bench/${task}`) {
    throw new Error(`trial task mismatch for ${task}`);
  }
  const config = JSON.parse(
    await readFile(path.join(trial.trialDir, "config.json"), "utf8"),
  );
  const mounts = config.environment?.mounts ?? [];
  const expectedSource = stateDir.replaceAll("\\", "/");
  const stateMount = mounts.find(
    (mount) => mount.target === "/tmp/stateful-codex-state",
  );
  if (stateMount?.source !== expectedSource || stateMount.type !== "bind") {
    throw new Error(`state mount mismatch for ${task}`);
  }
}

function isPreAgentInfrastructureFailure(result) {
  return Boolean(
    result.exception_info &&
      !result.agent_result &&
      !result.agent_execution?.started_at,
  );
}

async function buildFeedback(resultPath, attempt) {
  const result = JSON.parse(await readFile(resultPath, "utf8"));
  const reward = result.verifier_result?.rewards?.reward;
  const exception = result.exception_info;
  const parts = [
    `Authoritative attempt-${attempt} outcome: ${reward === 1 ? "PASS" : "FAIL"}${reward == null ? "" : ` (reward ${reward})`}.`,
  ];

  if (exception) {
    parts.push(
      `Execution exception: ${exception.exception_type ?? "unknown"}: ${exception.exception_message ?? "no message"}.`,
    );
  }

  const verifierDir = path.join(path.dirname(resultPath), "verifier");
  const ctrf = await readJsonIfPresent(path.join(verifierDir, "ctrf.json"));
  const tests = ctrf?.results?.tests ?? [];
  if (tests.length > 0) {
    const passed = tests.filter((test) => test.status === "passed").map((test) => test.name);
    const failed = tests.filter((test) => test.status === "failed");
    parts.push(
      `Verifier summary: ${passed.length} passed, ${failed.length} failed. Passed checks: ${passed.join(", ") || "none"}.`,
    );
    for (const test of failed) {
      const detail = [test.message, test.trace]
        .filter(Boolean)
        .join("\n")
        .slice(-2_500);
      parts.push(`Failed check ${test.name}: ${detail || "no diagnostic"}.`);
    }
  }

  return parts.join(" ").slice(0, MAX_FEEDBACK_CHARS);
}

async function observedBlackboardMutation(trialDir) {
  const sessions = path.join(trialDir, "agent", "sessions");
  const files = await recursiveFiles(sessions);
  for (const file of files) {
    if (!file.endsWith(".jsonl")) continue;
    const content = await readFile(file, "utf8");
    if (
      content.includes("tools.blackboard_record_batch") ||
      content.includes("tools.blackboard_update_batch")
    ) {
      return true;
    }
  }
  return false;
}

async function recordControllerOutcome(options, task, attempt, outcome) {
  const stateDirectory = path.join(options.stateRoot, "feedback-series-state");
  await mkdir(stateDirectory, { recursive: true });
  const statePath = path.join(stateDirectory, `${task}-a${attempt}.json`);
  const state = {
    protocol: "harbor-feedback-series-v1",
    task,
    attempt,
    ...outcome,
    recordedAt: new Date().toISOString(),
  };
  const temporary = `${statePath}.tmp`;
  await writeFile(temporary, `${JSON.stringify(state, null, 2)}\n`, "utf8");
  await rename(temporary, statePath);
}

async function findColdResult(entry, jobsDir) {
  const jobEntries = await readdir(jobsDir, { withFileTypes: true });
  for (const job of jobEntries) {
    if (!job.isDirectory()) continue;
    const candidate = path.join(jobsDir, job.name, entry.coldTrial, "result.json");
    if (await exists(candidate)) return candidate;
  }
  throw new Error(`cold result not found for ${entry.task}/${entry.coldTrial}`);
}

async function prioritizeStartedTasks(cohort, options) {
  const annotated = await Promise.all(
    cohort.map(async (entry, index) => {
      let active = false;
      let started = false;
      for (let attempt = 2; attempt <= options.attempts; attempt += 1) {
        const result = await readJsonIfPresent(
          path.join(
            options.jobsDir,
            `${options.jobPrefix}-${entry.task}-a${attempt}`,
            "result.json",
          ),
        );
        if (result) started = true;
        if (result && !result.finished_at) active = true;
      }
      return { entry, index, active, started };
    }),
  );
  return annotated
    .sort(
      (left, right) =>
        Number(right.active) - Number(left.active) ||
        Number(right.started) - Number(left.started) ||
        left.index - right.index,
    )
    .map(({ entry }) => entry);
}

async function recursiveFiles(root) {
  if (!(await exists(root))) return [];
  const output = [];
  for (const entry of await readdir(root, { withFileTypes: true })) {
    const candidate = path.join(root, entry.name);
    if (entry.isDirectory()) output.push(...(await recursiveFiles(candidate)));
    else if (entry.isFile()) output.push(candidate);
  }
  return output;
}

async function readJsonIfPresent(file) {
  try {
    return JSON.parse(await readFile(file, "utf8"));
  } catch (error) {
    if (error.code === "ENOENT") return null;
    throw error;
  }
}

async function exists(file) {
  try {
    await access(file);
    return true;
  } catch {
    return false;
  }
}

async function validateOptions(options) {
  for (const file of [options.cohort, options.harbor, options.bundle, options.auth]) {
    await access(file);
  }
  await mkdir(options.stateRoot, { recursive: true });
  await mkdir(options.jobsDir, { recursive: true });
}

function parseArgs(argv) {
  const values = new Map();
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!key?.startsWith("--") || value == null) {
      throw new Error(`invalid argument near ${key ?? "end of input"}`);
    }
    values.set(key.slice(2), value);
  }
  const required = [
    "cohort",
    "state-root",
    "jobs-dir",
    "harbor",
    "bundle",
    "bundle-sha256",
    "auth",
    "python-path",
    "dataset",
    "job-prefix",
  ];
  for (const key of required) {
    if (!values.has(key)) throw new Error(`missing --${key}`);
  }
  return {
    cohort: path.resolve(values.get("cohort")),
    stateRoot: path.resolve(values.get("state-root")),
    jobsDir: path.resolve(values.get("jobs-dir")),
    harbor: path.resolve(values.get("harbor")),
    bundle: path.resolve(values.get("bundle")),
    bundleSha256: values.get("bundle-sha256"),
    auth: path.resolve(values.get("auth")),
    pythonPath: path.resolve(values.get("python-path")),
    dataset: values.get("dataset"),
    jobPrefix: values.get("job-prefix"),
    model: values.get("model") ?? "openai/gpt-5.6-luna",
    effort: values.get("effort") ?? "max",
    attempts: Number(values.get("attempts") ?? 5),
    concurrency: Number(values.get("concurrency") ?? 8),
    jobTimeoutMs: Number(values.get("job-timeout-ms") ?? 14_400_000),
    cwd: path.resolve(values.get("cwd") ?? process.cwd()),
  };
}

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

class Semaphore {
  constructor(limit) {
    if (!Number.isInteger(limit) || limit < 1) {
      throw new Error("concurrency must be a positive integer");
    }
    this.available = limit;
    this.waiters = [];
  }

  async acquire() {
    if (this.available > 0) {
      this.available -= 1;
    } else {
      await new Promise((resolve) => this.waiters.push(resolve));
    }
    return () => {
      const next = this.waiters.shift();
      if (next) next();
      else this.available += 1;
    };
  }
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
