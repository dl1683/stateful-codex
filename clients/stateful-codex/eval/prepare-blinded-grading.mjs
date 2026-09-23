import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { readEvents } from "./compare-rollouts.mjs";
import { resolveRolloutPath } from "./rollout-path.mjs";
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

export function armForLabel(projectId, seed, label) {
  const statefulIsA =
    createHash("sha256").update(`${seed}\0${projectId}`).digest()[0] % 2 === 0;
  if (label === "A") return statefulIsA ? "stateful" : "baseline";
  if (label === "B") return statefulIsA ? "baseline" : "stateful";
  throw new Error(`unknown blinded label: ${label}`);
}

export function redactArmPaths(value, snapshotRoot, projectId) {
  let redacted = value;
  redacted = redacted.replace(
    /[A-Za-z]:[\\/][^<>"\r\n]*?[\\/](?:baseline|stateful)(?=[\\/])/gi,
    "PROJECT_ROOT",
  );
  const snapshotPattern = escapeRegExp(snapshotRoot).replaceAll("\\\\", "[\\\\/]");
  redacted = redacted.replace(
    new RegExp(`${snapshotPattern}[\\\\/][^\\\\/]+[\\\\/](?:baseline|stateful)`, "gi"),
    "PROJECT_ROOT",
  );
  for (const arm of ["baseline", "stateful"]) {
    const armRoot = path.join(snapshotRoot, projectId, arm);
    const variants = [armRoot, armRoot.replaceAll("\\", "/")];
    for (const variant of variants) redacted = redacted.replaceAll(variant, "PROJECT_ROOT");
  }
  return redacted;
}

export function gradingOutputsOverlap(publicOutput, privateOutput) {
  const publicPath = path.resolve(publicOutput);
  const privatePath = path.resolve(privateOutput);
  return containsPath(publicPath, privatePath) || containsPath(privatePath, publicPath);
}

function containsPath(parent, child) {
  const relative = path.relative(parent, child);
  return relative === "" || (!relative.startsWith("..") && !path.isAbsolute(relative));
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const manifest = JSON.parse(await readFile(options.manifest, "utf8"));
  const mapping = {
    rubricVersion: RUBRIC_VERSION,
    seed: options.seed,
    projects: {},
  };
  if (gradingOutputsOverlap(options.output, options.mappingOutput)) {
    throw new Error(
      "public grading packets and the private arm mapping must use disjoint directories",
    );
  }
  await mkdir(options.output, { recursive: true });
  await mkdir(options.mappingOutput, { recursive: true });

  for (const project of manifest.projects) {
    const arms = {};
    for (const arm of ["baseline", "stateful"]) {
      const statePath = path.join(options.resultRoot, project.id, arm, "run-state.json");
      const state = JSON.parse(await readFile(statePath, "utf8"));
      const rolloutPath = await resolveRolloutPath(state, options.sessionsRoot);
      const summary = summarizeLongitudinalEvents(await readEvents(rolloutPath));
      if (summary.sessionId !== state.threadId) {
        throw new Error(`rollout thread mismatch for ${project.id}/${arm}`);
      }
      if (summary.issues.length > 0 || summary.turns.length !== project.cases.length) {
        throw new Error(`invalid rollout for ${project.id}/${arm}`);
      }
      arms[arm] = { state, summary, rolloutPath };
    }

    const labels = Object.fromEntries(
      ["A", "B"].map((label) => [label, armForLabel(project.id, options.seed, label)]),
    );
    const benchmarkCase = project.cases[0];
    const packet = {
      rubricVersion: RUBRIC_VERSION,
      artifact: "visibleAnswers",
      project: {
        id: project.id,
        domain: project.domain,
        prompt: benchmarkCase.prompt,
        corpusRevision: arms.baseline.state.turns[0].corpusRevision,
        sourceRoot: path.join(options.snapshotRoot, project.id, "baseline"),
      },
      instructions: {
        blinded: true,
        scoreRange: "0-4 integer per dimension",
        dimensions: DIMENSIONS,
        compareVisibleAnswers: ["A", "B"],
        requirements: [
          "Verify consequential claims against the supplied source corpus.",
          "Do not infer product arm identity or use token, latency, or tool-count evidence.",
          "For every dimension, provide a score and concise evidence-backed rationale.",
          "Identify material unsupported claims, missed decisive details, and citation failures.",
          "Choose A, B, or tie only after scoring each answer independently.",
        ],
      },
      visibleAnswers: Object.fromEntries(
        Object.entries(labels).map(([label, arm]) => [
          label,
          redactArmPaths(
            arms[arm].summary.turns[0].finalAnswer,
            options.snapshotRoot,
            project.id,
          ),
        ]),
      ),
    };
    const serializedPacket = `${JSON.stringify(packet, null, 2)}\n`;
    mapping.projects[project.id] = {
      labels,
      packetSha256: createHash("sha256").update(serializedPacket).digest("hex"),
      rollouts: {
        baseline: arms.baseline.rolloutPath,
        stateful: arms.stateful.rolloutPath,
      },
    };
    await writeFile(
      path.join(options.output, `${project.id}.packet.json`),
      serializedPacket,
    );
  }

  await writeFile(
    path.join(options.mappingOutput, "grading-map.json"),
    `${JSON.stringify(mapping, null, 2)}\n`,
  );
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
    else if (argument === "--mapping-output") options.mappingOutput = value;
    else if (argument === "--seed") options.seed = value;
    else throw new Error(`unknown argument: ${argument}`);
  }
  const required = [
    "manifest",
    "resultRoot",
    "snapshotRoot",
    "sessionsRoot",
    "output",
    "mappingOutput",
    "seed",
  ];
  if (required.some((field) => !options[field])) {
    throw new Error(
      "usage: --manifest PATH --result-root PATH --snapshot-root PATH --output PATH --mapping-output PATH --seed VALUE [--sessions-root PATH]",
    );
  }
  for (const field of required.filter((field) => field !== "seed")) {
    options[field] = path.resolve(options[field]);
  }
  if (options.sessionsRoot) options.sessionsRoot = path.resolve(options.sessionsRoot);
  return options;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
