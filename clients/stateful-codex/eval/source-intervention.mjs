import { createHash } from "node:crypto";
import { readFile, rename, writeFile } from "node:fs/promises";
import path from "node:path";

import { corpusHash } from "./corpus-hash.mjs";

export async function applyScheduledIntervention({
  workspace,
  manifestDirectory,
  intervention,
  state,
}) {
  if (!intervention) {
    const current = await corpusHash(workspace);
    if (`sha256:${current.sha256}` !== state.corpusRevision) {
      throw new Error("unplanned corpus change before the next case");
    }
    return { applied: false, record: null };
  }
  const existing = state.appliedInterventions.find(
    (candidate) => candidate.id === intervention.id,
  );
  if (existing) {
    const current = await corpusHash(workspace);
    if (`sha256:${current.sha256}` !== existing.afterCorpusRevision) {
      throw new Error(`corpus drift after intervention ${intervention.id}`);
    }
    state.corpusRevision = existing.afterCorpusRevision;
    return { applied: false, record: existing };
  }
  validateIntervention(intervention);
  if (state.corpusRevision !== intervention.expectedBeforeCorpusRevision) {
    throw new Error(`intervention ${intervention.id} has an unexpected predecessor`);
  }
  const before = await corpusHash(workspace);
  if (`sha256:${before.sha256}` !== intervention.expectedBeforeCorpusRevision) {
    throw new Error(`corpus changed before intervention ${intervention.id}`);
  }
  const replacements = [];
  for (const file of intervention.files) {
    const target = resolveWithin(workspace, file.path);
    const source = path.resolve(manifestDirectory, file.replacement);
    const [current, replacement] = await Promise.all([
      readFile(target),
      readFile(source),
    ]);
    if (sha256(current) !== file.expectedBeforeSha256) {
      throw new Error(`unexpected source revision for ${file.path}`);
    }
    if (sha256(replacement) !== file.replacementSha256) {
      throw new Error(`replacement hash mismatch for ${file.path}`);
    }
    replacements.push({ file, target, current, replacement });
  }
  try {
    for (const replacement of replacements) {
      const temporaryPath = `${replacement.target}.stateful-revision-${process.pid}`;
      await writeFile(temporaryPath, replacement.replacement);
      await rename(temporaryPath, replacement.target);
    }
    const after = await corpusHash(workspace);
    if (`sha256:${after.sha256}` !== intervention.expectedAfterCorpusRevision) {
      throw new Error(`intervention ${intervention.id} produced an unexpected corpus`);
    }
  } catch (error) {
    await Promise.all(
      replacements.map((replacement) => writeFile(replacement.target, replacement.current)),
    );
    throw error;
  }
  const record = {
    id: intervention.id,
    appliedAtMs: Date.now(),
    beforeCorpusRevision: intervention.expectedBeforeCorpusRevision,
    afterCorpusRevision: intervention.expectedAfterCorpusRevision,
    files: intervention.files.map((file) => ({
      path: file.path,
      beforeSha256: file.expectedBeforeSha256,
      afterSha256: file.replacementSha256,
    })),
  };
  state.appliedInterventions.push(record);
  state.corpusRevision = record.afterCorpusRevision;
  return { applied: true, record };
}

export async function validateInterventionSchedule(
  cases,
  initialCorpusRevision,
  manifestDirectory,
) {
  let expectedRevision = initialCorpusRevision;
  const ids = new Set();
  const schedule = [];
  for (const benchmarkCase of cases) {
    const intervention = benchmarkCase.intervention;
    if (!intervention) continue;
    validateIntervention(intervention);
    if (ids.has(intervention.id)) {
      throw new Error(`duplicate source intervention ID: ${intervention.id}`);
    }
    if (intervention.expectedBeforeCorpusRevision !== expectedRevision) {
      throw new Error(`source intervention chain breaks before ${intervention.id}`);
    }
    for (const file of intervention.files) {
      const replacement = await readFile(path.resolve(manifestDirectory, file.replacement));
      if (sha256(replacement) !== file.replacementSha256) {
        throw new Error(`replacement hash mismatch for ${file.path}`);
      }
    }
    ids.add(intervention.id);
    expectedRevision = intervention.expectedAfterCorpusRevision;
    schedule.push({
      caseId: benchmarkCase.id,
      id: intervention.id,
      beforeCorpusRevision: intervention.expectedBeforeCorpusRevision,
      afterCorpusRevision: intervention.expectedAfterCorpusRevision,
      files: intervention.files.map((file) => ({
        path: file.path,
        beforeSha256: file.expectedBeforeSha256,
        afterSha256: file.replacementSha256,
      })),
    });
  }
  return schedule;
}

function validateIntervention(intervention) {
  if (
    typeof intervention.id !== "string" ||
    !intervention.id ||
    !/^sha256:[0-9a-f]{64}$/i.test(intervention.expectedBeforeCorpusRevision ?? "") ||
    !/^sha256:[0-9a-f]{64}$/i.test(intervention.expectedAfterCorpusRevision ?? "") ||
    !Array.isArray(intervention.files) ||
    intervention.files.length === 0
  ) {
    throw new Error("source intervention is incomplete");
  }
  const paths = new Set();
  for (const file of intervention.files) {
    if (
      typeof file.path !== "string" ||
      !file.path ||
      typeof file.replacement !== "string" ||
      !file.replacement ||
      !/^[0-9a-f]{64}$/i.test(file.expectedBeforeSha256 ?? "") ||
      !/^[0-9a-f]{64}$/i.test(file.replacementSha256 ?? "") ||
      paths.has(file.path)
    ) {
      throw new Error(`invalid source intervention file: ${file.path ?? "unknown"}`);
    }
    paths.add(file.path);
  }
}

function resolveWithin(root, relativePath) {
  const target = path.resolve(root, relativePath);
  const relative = path.relative(root, target);
  if (!relative || relative.startsWith("..") || path.isAbsolute(relative)) {
    throw new Error(`source intervention path escapes the workspace: ${relativePath}`);
  }
  return target;
}

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}
