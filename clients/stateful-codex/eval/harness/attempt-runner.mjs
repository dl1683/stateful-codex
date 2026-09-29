import { createHash } from "node:crypto";
import { createWriteStream } from "node:fs";
import { cp, mkdir, readFile, readdir, rename, stat, writeFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import path from "node:path";
import { isolatedEnvironment, prepareIsolatedHome } from "./isolated-home.mjs";

export async function runAttempt({ attemptRoot, scenario, codex, fixtureRoot, authHome, args = [], timeoutSeconds = scenario.conversation.turnTimeoutSeconds }) {
  const startedAt = new Date().toISOString();
  const workspace = path.join(attemptRoot, "workspace");
  const evidence = path.join(attemptRoot, "evidence");
  await mkdir(evidence, { recursive: true });
  if (path.resolve(workspace) === path.resolve(fixtureRoot)) throw new Error("fixture source cannot be the output workspace");
  await cp(fixtureRoot, workspace, { recursive: true });
  const homes = await prepareIsolatedHome(attemptRoot, authHome);
  const executableSha256 = `sha256:${createHash("sha256").update(await readFile(codex)).digest("hex")}`;
  const stdoutPath = path.join(attemptRoot, "stdout.txt");
  const stderrPath = path.join(attemptRoot, "stderr.txt");
  const stdout = createWriteStream(stdoutPath);
  const stderr = createWriteStream(stderrPath);
  const child = spawn(codex, args, { cwd: workspace, env: isolatedEnvironment(homes), windowsHide: true });
  child.stdout.pipe(stdout);
  child.stderr.pipe(stderr);
  const outputDone = Promise.all([finished(stdout), finished(stderr)]);
  const result = await waitForChild(child, timeoutSeconds * 1000);
  await outputDone;
  const manifest = {
    schemaVersion: 1,
    scenarioId: scenario.id,
    scenarioSha256: `sha256:${createHash("sha256").update(JSON.stringify(scenario)).digest("hex")}`,
    fixtureSha256: scenario.fixture.sha256,
    executableSha256,
    startedAt,
    finishedAt: new Date().toISOString(),
    process: { pid: child.pid },
    exitReason: result.exitReason,
    exitCode: result.code,
    signal: result.signal,
    retryClassification: classifyRetry(result.exitReason, await readFile(stderrPath, "utf8")),
    evidence: await inventory(attemptRoot),
  };
  await atomicJson(path.join(attemptRoot, "attempt.json"), manifest);
  return manifest;
}

function waitForChild(child, timeout) {
  return new Promise((resolve) => {
    let timer = setTimeout(() => {
      child.kill("SIGINT");
      setTimeout(() => child.kill(), 1000).unref();
      resolve({ exitReason: "timedOut", code: null, signal: "SIGINT" });
    }, timeout);
    child.once("error", (error) => { clearTimeout(timer); resolve({ exitReason: "spawnError", code: null, signal: error.code }); });
    child.once("exit", (code, signal) => { clearTimeout(timer); resolve({ exitReason: "processExited", code, signal }); });
  });
}

function classifyRetry(reason, stderr) {
  if (reason === "spawnError" || /capacity|transport|connection reset|429/i.test(stderr)) return "infrastructure";
  return "none";
}

async function inventory(root) {
  const result = [];
  async function visit(directory, prefix = "") {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const relative = path.join(prefix, entry.name);
      if (relative === "attempt.json") continue;
      if (entry.isDirectory()) await visit(path.join(directory, entry.name), relative);
      else result.push(relative.replaceAll(path.sep, "/"));
    }
  }
  await visit(root);
  return result.sort();
}

async function atomicJson(file, value) {
  const temporary = `${file}.tmp-${process.pid}`;
  await writeFile(temporary, `${JSON.stringify(value, null, 2)}\n`);
  await rename(temporary, file);
}

function finished(stream) { return new Promise((resolve) => stream.once("finish", resolve)); }
