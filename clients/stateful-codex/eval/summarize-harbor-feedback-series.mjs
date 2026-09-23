import { createHash } from "node:crypto";
import { access, readFile, readdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const cohort = JSON.parse(await readFile(options.cohort, "utf8"));
  const tasks = [];
  const missing = [];

  for (const entry of cohort) {
    const attempts = [];
    const coldResult = await findColdResult(entry, options.jobsDir);
    attempts.push(await summarizeAttempt(coldResult, 1, null, options.jobsDir));
    for (let attempt = 2; attempt <= options.attempts; attempt += 1) {
      const controllerPath = path.join(
        options.stateRoot,
        "feedback-series-state",
        `${entry.task}-a${attempt}.json`,
      );
      const controller = await readJsonIfPresent(controllerPath);
      if (!controller) {
        missing.push({ task: entry.task, attempt });
        continue;
      }
      attempts.push(
        await summarizeAttempt(
          controller.resultPath,
          attempt,
          controller,
          options.jobsDir,
        ),
      );
    }
    tasks.push({
      task: entry.task,
      trajectory: summarizeTrajectory(attempts),
      attempts,
    });
  }

  const completed = tasks.flatMap((task) => task.attempts);
  const byAttempt = [];
  for (let attempt = 1; attempt <= options.attempts; attempt += 1) {
    const records = completed.filter((record) => record.attempt === attempt);
    byAttempt.push(aggregate(records, attempt));
  }
  const report = {
    schemaVersion: 1,
    generatedAt: new Date().toISOString(),
    complete: missing.length === 0,
    expectedTasks: cohort.length,
    expectedAttemptsPerTask: options.attempts,
    completedAttempts: completed.length,
    missing,
    aggregate: aggregate(completed, null),
    byAttempt,
    tasks,
  };
  const serialized = `${JSON.stringify(report, null, 2)}\n`;
  if (options.output) await writeFile(options.output, serialized, "utf8");
  else process.stdout.write(serialized);
  if (missing.length > 0 && !options.allowIncomplete) process.exitCode = 2;
}

async function summarizeAttempt(
  resultPath,
  attempt,
  controller,
  jobsDir,
) {
  const result = JSON.parse(await readFile(resultPath, "utf8"));
  const trialDir = path.dirname(resultPath);
  const task = result.task_name?.replace(/^terminal-bench\//, "");
  const tests = await verifierTests(trialDir);
  const input = result.agent_result?.n_input_tokens ?? null;
  const cached = result.agent_result?.n_cache_tokens ?? null;
  return {
    attempt,
    protocol: controller?.protocol ?? "cold-independent",
    task,
    job: relativeOrBasename(jobsDir, path.dirname(trialDir)),
    trial: path.basename(trialDir),
    reward: result.verifier_result?.rewards?.reward ?? null,
    exceptionType: result.exception_info?.exception_type ?? null,
    inputTokens: input,
    cachedInputTokens: cached,
    uncachedInputTokens: input == null || cached == null ? null : input - cached,
    outputTokens: result.agent_result?.n_output_tokens ?? null,
    costUsd: result.agent_result?.cost_usd ?? null,
    agentSeconds: durationSeconds(
      result.agent_execution?.started_at,
      result.agent_execution?.finished_at,
    ),
    totalSeconds: durationSeconds(result.started_at, result.finished_at),
    memoryMutationObserved: controller?.memoryMutationObserved ?? null,
    infrastructureRetries: controller?.infrastructureRetries ?? 0,
    resultSha256: sha256(await readFile(resultPath)),
    verifier: {
      passed: tests.filter((test) => test.status === "passed").length,
      failed: tests.filter((test) => test.status === "failed").length,
      tests,
    },
  };
}

async function verifierTests(trialDir) {
  const ctrf = await readJsonIfPresent(
    path.join(trialDir, "verifier", "ctrf.json"),
  );
  return (ctrf?.results?.tests ?? []).map((test) => ({
    name: test.name,
    status: test.status,
    durationSeconds: test.duration ?? null,
    message: test.status === "failed" ? test.message ?? null : null,
    traceTail:
      test.status === "failed" && test.trace
        ? test.trace.slice(-2_500)
        : null,
  }));
}

function aggregate(records, attempt) {
  const numeric = (field) => records.map((record) => record[field]).filter(Number.isFinite);
  const sum = (values) => values.reduce((total, value) => total + value, 0);
  const passes = records.filter((record) => record.reward === 1).length;
  return {
    attempt,
    records: records.length,
    passes,
    failures: records.length - passes,
    passRate: records.length === 0 ? null : passes / records.length,
    inputTokens: sum(numeric("inputTokens")),
    cachedInputTokens: sum(numeric("cachedInputTokens")),
    uncachedInputTokens: sum(numeric("uncachedInputTokens")),
    outputTokens: sum(numeric("outputTokens")),
    costUsd: sum(numeric("costUsd")),
    agentSeconds: sum(numeric("agentSeconds")),
    totalSeconds: sum(numeric("totalSeconds")),
    persistenceMisses: records.filter(
      (record) => record.attempt > 1 && record.memoryMutationObserved === false,
    ).length,
    infrastructureRetries: sum(numeric("infrastructureRetries")),
  };
}

function summarizeTrajectory(attempts) {
  const transitions = attempts.slice(1).map((attempt, index) => ({
    from: attempts[index].reward,
    to: attempt.reward,
  }));
  const costs = attempts.map((attempt) => attempt.costUsd);
  const coldCostUsd = costs[0] ?? null;
  const finalCostUsd = costs.at(-1) ?? null;
  const passingCosts = attempts
    .filter((attempt) => attempt.reward === 1)
    .map((attempt) => attempt.costUsd)
    .filter(Number.isFinite);
  return {
    rewards: attempts.map((attempt) => attempt.reward),
    coldReward: attempts[0]?.reward ?? null,
    finalReward: attempts.at(-1)?.reward ?? null,
    warmPasses: attempts.slice(1).filter((attempt) => attempt.reward === 1)
      .length,
    warmFailures: attempts.slice(1).filter((attempt) => attempt.reward !== 1)
      .length,
    passToFailTransitions: transitions.filter(
      (transition) => transition.from === 1 && transition.to !== 1,
    ).length,
    failToPassTransitions: transitions.filter(
      (transition) => transition.from !== 1 && transition.to === 1,
    ).length,
    coldCostUsd,
    finalCostUsd,
    finalToColdCostRatio:
      Number.isFinite(coldCostUsd) &&
      coldCostUsd > 0 &&
      Number.isFinite(finalCostUsd)
        ? finalCostUsd / coldCostUsd
        : null,
    lowestPassingCostUsd:
      passingCosts.length === 0 ? null : Math.min(...passingCosts),
  };
}

async function findColdResult(entry, jobsDir) {
  for (const job of await readdir(jobsDir, { withFileTypes: true })) {
    if (!job.isDirectory()) continue;
    const candidate = path.join(jobsDir, job.name, entry.coldTrial, "result.json");
    if (await exists(candidate)) return candidate;
  }
  throw new Error(`cold result not found for ${entry.task}/${entry.coldTrial}`);
}

function durationSeconds(start, finish) {
  if (!start || !finish) return null;
  return (new Date(finish).getTime() - new Date(start).getTime()) / 1_000;
}

function relativeOrBasename(root, target) {
  const relative = path.relative(root, target);
  return relative && !relative.startsWith("..") ? relative : path.basename(target);
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
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
  for (const key of ["cohort", "state-root", "jobs-dir"]) {
    if (!values.has(key)) throw new Error(`missing --${key}`);
  }
  return {
    cohort: path.resolve(values.get("cohort")),
    stateRoot: path.resolve(values.get("state-root")),
    jobsDir: path.resolve(values.get("jobs-dir")),
    output: values.has("output") ? path.resolve(values.get("output")) : null,
    attempts: Number(values.get("attempts") ?? 5),
    allowIncomplete: values.get("allow-incomplete") === "true",
  };
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
