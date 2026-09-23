import { createHash } from "node:crypto";
import { mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { readEvents } from "./compare-rollouts.mjs";
import { stateArtifactHash } from "./export-project-state.mjs";
import { summarizeLongitudinalEvents } from "./summarize-longitudinal-rollout.mjs";

const RUBRIC_VERSION = "stateful-longitudinal-v1";
const DIMENSIONS = [
  "correctness",
  "evidenceTraceability",
  "decisiveDetail",
  "uncertaintyCalibration",
  "contradictionAndFreshness",
  "usefulness",
];

const ARTIFACTS = [
  ["submittedNarrative", "The model-submitted completion narrative before durable completion enrichment."],
  ["persistedRunResult", "The exact result stored on the completed Stateful run."],
  ["semanticObligation", "The latest structured semantic obligation stored for this run."],
  ["projectIntelligence", "The exact persisted hierarchy, context map, blackboard revisions, evidence links, and relations."],
];

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const manifest = JSON.parse(await readFile(options.manifest, "utf8"));
  const sessionFiles = await readdir(options.sessionsRoot);
  const gradingManifest = {
    rubricVersion: RUBRIC_VERSION,
    artifactSeparation: true,
    projects: {},
  };
  await mkdir(options.output, { recursive: true });

  for (const project of manifest.projects) {
    const runState = JSON.parse(
      await readFile(
        path.join(options.resultRoot, project.id, "stateful", "run-state.json"),
        "utf8",
      ),
    );
    const rolloutName = sessionFiles.find((name) => name.includes(runState.threadId));
    if (!rolloutName) throw new Error(`missing Stateful rollout for ${project.id}`);
    const summary = summarizeLongitudinalEvents(
      await readEvents(path.join(options.sessionsRoot, rolloutName)),
    );
    if (summary.issues.length > 0 || summary.turns.length !== project.cases.length) {
      throw new Error(`invalid Stateful rollout for ${project.id}`);
    }
    if (runState.turns.length !== project.cases.length) {
      throw new Error(`incomplete Stateful run state for ${project.id}`);
    }

    const projectManifest = { cases: {} };
    gradingManifest.projects[project.id] = projectManifest;
    for (const [index, benchmarkCase] of project.cases.entries()) {
      const turn = runState.turns[index];
      if (turn.id !== benchmarkCase.id || !turn.stateArtifact) {
        throw new Error(`missing state artifact for ${project.id}/${benchmarkCase.id}`);
      }
      const state = JSON.parse(await readFile(turn.stateArtifact.path, "utf8"));
      verifyStateArtifact(state, turn);
      const values = {
        submittedNarrative: summary.turns[index].durableCompletion?.submittedResult ?? null,
        persistedRunResult: state.run.result,
        semanticObligation: state.obligations.at(-1) ?? null,
        projectIntelligence: {
          projectId: state.projectId,
          intelligenceRevision: state.intelligenceRevision,
          counts: state.counts,
          hierarchy: state.hierarchy,
          contextMap: state.contextMap,
          blackboard: state.blackboard,
        },
      };
      if (Object.values(values).some((value) => value == null)) {
        throw new Error(`incomplete durable artifacts for ${project.id}/${benchmarkCase.id}`);
      }
      const caseDirectory = path.join(options.output, project.id, benchmarkCase.id);
      await mkdir(caseDirectory, { recursive: true });
      const caseManifest = {
        corpusRevision: turn.corpusRevision,
        stateSnapshotSha256: state.snapshotSha256,
        packets: {},
      };
      projectManifest.cases[benchmarkCase.id] = caseManifest;
      for (const [artifact, description] of ARTIFACTS) {
        const packet = {
          rubricVersion: RUBRIC_VERSION,
          artifact,
          project: {
            id: project.id,
            domain: project.domain,
            prompt: benchmarkCase.prompt,
            corpusRevision: turn.corpusRevision,
            sourceRoot: path.join(options.snapshotRoot, project.id, "baseline"),
          },
          instructions: {
            blindedComparison: false,
            independentArtifact: true,
            description,
            scoreRange: "0-4 integer per dimension",
            dimensions: DIMENSIONS,
            requirements: [
              "Judge only this artifact; do not use another artifact's quality as a proxy.",
              "Verify consequential claims against the supplied source corpus.",
              "Reference exact entry, evidence, relation, obligation, or run IDs when available.",
              "Identify lost qualifications, stale claims, unsupported promotion, and missing uncertainty.",
            ],
          },
          value: values[artifact],
          provenance: {
            stateSnapshotSha256: state.snapshotSha256,
            capturedAtMs: state.capturedAtMs,
            intelligenceRevision: state.intelligenceRevision,
            runId: state.run.id,
            runRevision: state.run.revision,
          },
        };
        const serialized = `${JSON.stringify(packet, null, 2)}\n`;
        const filename = `${artifact}.packet.json`;
        await writeFile(path.join(caseDirectory, filename), serialized);
        caseManifest.packets[artifact] = {
          path: path.join(project.id, benchmarkCase.id, filename),
          sha256: createHash("sha256").update(serialized).digest("hex"),
        };
      }
    }
  }

  await writeFile(
    path.join(options.output, "state-grading-manifest.json"),
    `${JSON.stringify(gradingManifest, null, 2)}\n`,
  );
}

export function verifyStateArtifact(state, turn) {
  if (state.snapshotSha256 !== turn.stateArtifact.snapshotSha256) {
    throw new Error("run state and state artifact hashes differ");
  }
  if (state.snapshotSha256 !== stateArtifactHash(state)) {
    throw new Error("state artifact content does not match its hash");
  }
  if (state.corpusRevision !== turn.corpusRevision) {
    throw new Error("state artifact corpus revision does not match the turn");
  }
  if (state.capturedAtMs !== turn.stateArtifact.capturedAtMs) {
    throw new Error("state artifact capture time does not match the turn");
  }
  if (state.intelligenceRevision !== turn.stateArtifact.intelligenceRevision) {
    throw new Error("state artifact intelligence revision does not match the turn");
  }
  if (state.run.revision !== turn.stateArtifact.runRevision) {
    throw new Error("state artifact run revision does not match the turn");
  }
}

function parseArgs(args) {
  const options = {};
  for (let index = 0; index < args.length; index += 2) {
    const [argument, value] = args.slice(index, index + 2);
    if (argument === "--manifest") options.manifest = value;
    else if (argument === "--result-root") options.resultRoot = value;
    else if (argument === "--snapshot-root") options.snapshotRoot = value;
    else if (argument === "--sessions-root") options.sessionsRoot = value;
    else if (argument === "--output") options.output = value;
    else throw new Error(`unknown argument: ${argument}`);
  }
  const required = ["manifest", "resultRoot", "snapshotRoot", "sessionsRoot", "output"];
  if (required.some((field) => !options[field])) {
    throw new Error(
      "usage: --manifest PATH --result-root PATH --snapshot-root PATH --sessions-root PATH --output PATH",
    );
  }
  for (const field of required) options[field] = path.resolve(options[field]);
  return options;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
