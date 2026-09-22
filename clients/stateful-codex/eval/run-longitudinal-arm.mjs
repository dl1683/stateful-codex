import { spawn } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";

import { corpusHash } from "./corpus-hash.mjs";

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const manifest = JSON.parse(await readFile(options.manifest, "utf8"));
  const project = manifest.projects.find((candidate) => candidate.id === options.project);
  if (!project) throw new Error(`unknown project: ${options.project}`);
  const snapshot = JSON.parse(
    await readFile(path.join(options.snapshotRoot, project.id, "snapshot.json"), "utf8"),
  );
  const workspace = snapshot[options.arm];
  const resultRoot = path.join(options.output, project.id, options.arm);
  await mkdir(resultRoot, { recursive: true });
  const statePath = path.join(resultRoot, "run-state.json");
  const state = await readState(statePath, project.id, options.arm);
  const initialHash = await corpusHash(workspace);
  if (initialHash.sha256 !== snapshot.corpus.sha256) {
    throw new Error(`corpus changed before ${project.id}/${options.arm}`);
  }
  await assertChatGptLogin(options.codex, workspace);

  for (let index = state.nextCase; index < project.cases.length; index += 1) {
    const benchmarkCase = project.cases[index];
    console.error(`[${project.id}/${options.arm}] ${benchmarkCase.id}`);
    const output = await runTurn({
      ...options,
      workspace,
      resultRoot,
      index,
      prompt: benchmarkCase.prompt,
      threadId: state.threadId,
      mode: manifest.mode ?? "autonomous",
      model: manifest.model,
      reasoningEffort: manifest.reasoningEffort ?? "high",
    });
    state.threadId ??= output.threadId;
    if (!state.threadId) throw new Error("Codex output did not report a thread ID");
    const currentHash = await corpusHash(workspace);
    if (currentHash.sha256 !== snapshot.corpus.sha256) {
      throw new Error(`corpus changed during ${project.id}/${options.arm}/${benchmarkCase.id}`);
    }
    const turnRecord = {
      id: benchmarkCase.id,
      exitCode: output.exitCode,
      corpusRevision: `sha256:${currentHash.sha256}`,
    };
    state.turns.push(turnRecord);
    if (output.exitCode !== 0) {
      await writeFile(statePath, `${JSON.stringify(state, null, 2)}\n`);
      throw new Error(`Codex exited ${output.exitCode}`);
    }
    state.nextCase = index + 1;
    await writeFile(statePath, `${JSON.stringify(state, null, 2)}\n`);
  }
}

async function runTurn(options) {
  const turn = String(options.index + 1).padStart(2, "0");
  const args = options.threadId
    ? resumeArgs(options)
    : startArgs(options);
  const child = spawn(options.codex, args, {
    cwd: options.workspace,
    env: authEnvironment(),
    windowsHide: true,
  });
  child.stdin.end();
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (chunk) => {
    stdout += chunk;
    process.stdout.write(chunk);
  });
  child.stderr.on("data", (chunk) => {
    stderr += chunk;
    process.stderr.write(chunk);
  });
  const exitCode = await new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", resolve);
  });
  await Promise.all([
    writeFile(path.join(options.resultRoot, `turn-${turn}.stdout.jsonl`), stdout),
    writeFile(path.join(options.resultRoot, `turn-${turn}.stderr.txt`), stderr),
  ]);
  return { exitCode, threadId: options.threadId ?? extractThreadId(stdout) };
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
    "--sandbox",
    "read-only",
    "-C",
    options.workspace,
  ];
  if (options.arm === "stateful") args.push("--stateful", options.mode);
  args.push(options.prompt);
  return args;
}

function resumeArgs(options) {
  const args = [
    "exec",
    "resume",
    "--json",
    "--skip-git-repo-check",
    "--model",
    options.model,
    "-c",
    `model_reasoning_effort=\"${options.reasoningEffort}\"`,
    "-c",
    "sandbox_mode=\"read-only\"",
  ];
  if (options.arm === "stateful") args.push("--stateful", options.mode);
  args.push(options.threadId, options.prompt);
  return args;
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

async function assertChatGptLogin(codex, cwd) {
  const result = await capture(codex, ["login", "status"], cwd);
  if (
    result.exitCode !== 0 ||
    !`${result.stdout}\n${result.stderr}`.includes("Logged in using ChatGPT")
  ) {
    throw new Error("cached Codex ChatGPT login is required");
  }
}

async function capture(command, args, cwd) {
  const child = spawn(command, args, { cwd, env: authEnvironment(), windowsHide: true });
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

function authEnvironment() {
  const environment = { ...process.env };
  delete environment.OPENAI_API_KEY;
  delete environment.CODEX_API_KEY;
  return environment;
}

async function readState(statePath, project, arm) {
  try {
    return JSON.parse(await readFile(statePath, "utf8"));
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
    return { project, arm, threadId: null, nextCase: 0, turns: [] };
  }
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
      "usage: --manifest PATH --project ID --arm baseline|stateful --snapshot-root PATH --output PATH --codex PATH",
    );
  }
  for (const field of ["manifest", "snapshotRoot", "output", "codex"]) {
    options[field] = path.resolve(options[field]);
  }
  return options;
}

await main();
