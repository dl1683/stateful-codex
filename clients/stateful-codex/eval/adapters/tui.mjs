import { createWriteStream } from "node:fs";
import { mkdir, writeFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

export async function runTui({ attemptRoot, scenario, codex, python = process.env.PYTHON ?? "python", env = process.env }) {
  const evidence = path.join(attemptRoot, "evidence");
  await mkdir(evidence, { recursive: true });
  const conversation = path.join(attemptRoot, "conversation.json");
  await writeFile(conversation, JSON.stringify({ initialMessage: scenario.conversation.initialMessage, turns: scenario.conversation.turns }));
  const args = [path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../drivers/tui_conversation.py"), "--session", evidence, "--conversation", conversation, "--workspace", path.join(attemptRoot, "workspace"), "--codex", codex, "--mode", scenario.conversation.mode, "--turn-timeout", String(scenario.conversation.turnTimeoutSeconds), "--idle-stable", String(scenario.conversation.idleStableSeconds)];
  const stdout = createWriteStream(path.join(attemptRoot, "stdout.txt"));
  const stderr = createWriteStream(path.join(attemptRoot, "stderr.txt"));
  const tuiEnv = { ...env };
  delete tuiEnv.TERM;
  const child = spawn(python, args, { cwd: path.join(attemptRoot, "workspace"), env: tuiEnv, windowsHide: true });
  child.stdout.pipe(stdout); child.stderr.pipe(stderr);
  const outputDone = Promise.all([finished(stdout), finished(stderr)]);
  const [code, signal] = await new Promise((resolve) => child.once("exit", (exitCode, exitSignal) => resolve([exitCode, exitSignal])));
  await outputDone;
  return { code, signal, evidenceRoot: evidence };
}

export function tuiDriverArgs(scenario, attemptRoot, codex) {
  return { mode: scenario.conversation.mode, workspace: path.join(attemptRoot, "workspace"), codex };
}

function finished(stream) { return new Promise((resolve) => stream.once("finish", resolve)); }
