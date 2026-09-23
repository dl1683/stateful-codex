import { readdir, stat } from "node:fs/promises";
import path from "node:path";

export async function findRolloutPath(sessionsRoot, threadId) {
  const matches = [];
  const pending = [path.resolve(sessionsRoot)];
  while (pending.length > 0) {
    const directory = pending.pop();
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const candidate = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        pending.push(candidate);
      } else if (
        entry.isFile() &&
        entry.name.endsWith(`-${threadId}.jsonl`)
      ) {
        matches.push(candidate);
      }
    }
  }
  if (matches.length !== 1) {
    throw new Error(
      `expected one rollout for thread ${threadId}, found ${matches.length}`,
    );
  }
  return matches[0];
}

export async function resolveRolloutPath(runState, sessionsRoot) {
  if (runState.rolloutPath) {
    const rolloutPath = path.resolve(runState.rolloutPath);
    const metadata = await stat(rolloutPath);
    if (!metadata.isFile()) {
      throw new Error(`recorded rollout is not a file: ${rolloutPath}`);
    }
    if (!path.basename(rolloutPath).endsWith(`-${runState.threadId}.jsonl`)) {
      throw new Error(`recorded rollout does not match thread ${runState.threadId}`);
    }
    return rolloutPath;
  }
  if (!sessionsRoot) {
    throw new Error(`run state for thread ${runState.threadId} has no rolloutPath`);
  }
  return findRolloutPath(sessionsRoot, runState.threadId);
}
