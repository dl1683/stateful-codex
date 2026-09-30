import { createWriteStream } from "node:fs";
import { mkdir, writeFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

export async function runTui({ attemptRoot, workspace, scenario, codex, arm = "stateful", python = process.env.PYTHON ?? "python", env = process.env, timeoutSeconds }) {
  const evidence = path.join(attemptRoot, "evidence");
  await mkdir(evidence, { recursive: true });
  const conversation = path.join(attemptRoot, "conversation.json");
  await writeFile(conversation, JSON.stringify({ initialMessage: scenario.conversation.initialMessage, turns: scenario.conversation.turns }));
  const args = [path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../drivers/tui_conversation.py"), "--session", evidence, "--conversation", conversation, "--workspace", workspace, "--codex", codex, "--mode", scenario.conversation.mode, "--arm", arm, "--turn-timeout", String(scenario.conversation.turnTimeoutSeconds), "--idle-stable", String(scenario.conversation.idleStableSeconds)];
  const stdout = createWriteStream(path.join(attemptRoot, "stdout.txt"));
  const stderr = createWriteStream(path.join(attemptRoot, "stderr.txt"));
  const tuiEnv = { ...env };
  delete tuiEnv.TERM;
  const child = spawn(python, args, { cwd: workspace, env: tuiEnv, windowsHide: true });
  child.stdout.pipe(stdout); child.stderr.pipe(stderr);
  const outputDone = Promise.all([finished(stdout), finished(stderr)]);
  const result = await waitForChild(child, (timeoutSeconds ?? scenario.conversation.turnTimeoutSeconds) * 1000);
  await outputDone;
  return { ...result, evidenceRoot: evidence };
}

export function tuiDriverArgs(scenario, workspace, codex) {
  return { mode: scenario.conversation.mode, workspace, codex };
}

function finished(stream) { return new Promise((resolve) => stream.once("finish", resolve)); }

function waitForChild(child, timeout) {
  return new Promise((resolve) => {
    let settled = false;
    const finish = (result) => { if (!settled) { settled = true; clearTimeout(timer); resolve(result); } };
    const timer = setTimeout(() => {
      terminateProcessTree(child.pid);
      finish({ code: null, signal: "SIGTERM", exitReason: "timedOut", timedOut: true });
    }, timeout);
    child.once("error", (error) => finish({ code: null, signal: error.code, exitReason: "spawnError", error: error.message }));
    child.once("exit", (code, signal) => finish({ code, signal, exitReason: "processExited" }));
  });
}

function terminateProcessTree(pid) {
  if (!pid) return;
  if (process.platform === "win32") {
    const killer = spawn("taskkill", ["/PID", String(pid), "/T", "/F"], { windowsHide: true });
    killer.unref();
  } else {
    try { process.kill(-pid, "SIGTERM"); } catch { try { process.kill(pid, "SIGTERM"); } catch { /* already exited */ } }
  }
}
