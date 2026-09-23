import { createHash } from "node:crypto";
import { readFile, stat } from "node:fs/promises";
import path from "node:path";

const MAX_AUDITED_SOURCE_BYTES = 2 * 1024 * 1024;

export async function validateStateEvidenceAssertions({
  state,
  benchmarkCase,
  workspace,
}) {
  const assertions = benchmarkCase.stateEvidenceAssertions ?? [];
  if (!Array.isArray(assertions)) {
    throw new Error(
      `${benchmarkCase.id} stateEvidenceAssertions must be an array`,
    );
  }
  const currentRevisions = new Map(
    state.blackboard.entries.map((entry) => [entry.id, entry.revision]),
  );
  const revisions = state.blackboard.revisions.filter(
    (revision) => currentRevisions.get(revision.entryId) === revision.revision,
  );
  const contextMap = new Map(
    state.contextMap.map((entry) => [entry.id, entry]),
  );
  const hierarchy = new Map(state.hierarchy.map((node) => [node.id, node]));
  const results = [];

  for (const assertion of assertions) {
    validateAssertion(assertion, benchmarkCase.id);
    const candidates = revisions.filter(
      (revision) =>
        revision.verification === "sourceVerified" &&
        includesTermGroups(revision.content, assertion.entryTermGroups),
    );
    let matched = null;
    for (const revision of candidates) {
      const links = state.blackboard.evidenceLinks.filter(
        (link) =>
          link.entryId === revision.entryId &&
          link.revision === revision.revision,
      );
      for (const link of links) {
        const route = contextMap.get(link.contextMapEntryId);
        const node = route && hierarchy.get(route.nodeId);
        if (
          !route ||
          !node ||
          !Number.isInteger(link.firstLine) ||
          !Number.isInteger(link.lastLine)
        ) {
          continue;
        }
        if (
          link.sourceFingerprint !== route.sourceFingerprint ||
          node.lifecycle !== "active" ||
          node.sourceFingerprint !== route.sourceFingerprint
        ) {
          continue;
        }
        const source = await readSourceLines(workspace, node, link);
        if (
          source?.fingerprint === link.sourceFingerprint &&
          includesTermGroups(source.lines, assertion.sourceTermGroups)
        ) {
          matched = {
            name: assertion.name,
            entryId: revision.entryId,
            revision: revision.revision,
            contextMapEntryId: link.contextMapEntryId,
            relativePath: node.relativePath,
            firstLine: link.firstLine,
            lastLine: link.lastLine,
          };
          break;
        }
      }
      if (matched) break;
    }
    if (!matched) {
      throw new Error(
        `${benchmarkCase.id}/${assertion.name} has no current sourceVerified blackboard entry whose cited lines contain the decisive source terms`,
      );
    }
    results.push(matched);
  }
  return results;
}

function validateAssertion(assertion, caseId) {
  if (
    !assertion ||
    typeof assertion.name !== "string" ||
    !validTermGroups(assertion.entryTermGroups) ||
    !validTermGroups(assertion.sourceTermGroups)
  ) {
    throw new Error(`${caseId} contains an invalid state evidence assertion`);
  }
}

function validTermGroups(groups) {
  return (
    Array.isArray(groups) &&
    groups.length > 0 &&
    groups.every(
      (group) =>
        Array.isArray(group) &&
        group.length > 0 &&
        group.every((term) => typeof term === "string" && term.length > 0),
    )
  );
}

function includesTermGroups(value, groups) {
  const normalized = value.toLocaleLowerCase("en-US");
  return groups.every((group) =>
    group.some((term) => normalized.includes(term.toLocaleLowerCase("en-US"))),
  );
}

async function readSourceLines(workspace, node, link) {
  if (
    !node.projectRoot ||
    !node.relativePath ||
    link.firstLine > link.lastLine
  ) {
    return null;
  }
  if (!path.isAbsolute(node.projectRoot)) return null;
  const root = path.resolve(node.projectRoot);
  const relativeRoot = path.relative(path.resolve(workspace), root);
  if (
    relativeRoot === ".." ||
    relativeRoot.startsWith(`..${path.sep}`) ||
    path.isAbsolute(relativeRoot)
  ) {
    return null;
  }
  const sourcePath = path.resolve(root, node.relativePath);
  const relativeSource = path.relative(root, sourcePath);
  if (
    relativeSource === ".." ||
    relativeSource.startsWith(`..${path.sep}`) ||
    path.isAbsolute(relativeSource)
  ) {
    return null;
  }
  const metadata = await stat(sourcePath);
  if (!metadata.isFile() || metadata.size > MAX_AUDITED_SOURCE_BYTES)
    return null;
  const content = await readFile(sourcePath);
  const lines = content.toString("utf8").split(/\r?\n/);
  if (link.firstLine < 1 || link.lastLine > lines.length) return null;
  return {
    fingerprint: `sha256:${createHash("sha256").update(content).digest("hex")}`,
    lines: lines.slice(link.firstLine - 1, link.lastLine).join("\n"),
  };
}
