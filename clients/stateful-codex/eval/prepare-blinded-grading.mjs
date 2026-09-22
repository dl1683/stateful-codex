import { createHash } from "node:crypto";
import { mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { readEvents } from "./compare-rollouts.mjs";
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
  for (const arm of ["baseline", "stateful"]) {
    const armRoot = path.join(snapshotRoot, projectId, arm);
    const variants = [armRoot, armRoot.replaceAll("\\", "/")];
    for (const variant of variants) redacted = redacted.replaceAll(variant, "<PROJECT_ROOT>");
  }
  return redacted;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const manifest = JSON.parse(await readFile(options.manifest, "utf8"));
  const sessionFiles = await readdir(options.sessionsRoot);
  const mapping = {
    rubricVersion: RUBRIC_VERSION,
    seed: options.seed,
    projects: {},
  };
  await mkdir(options.output, { recursive: true });

  for (const project of manifest.projects) {
    const arms = {};
    for (const arm of ["baseline", "stateful"]) {
      const statePath = path.join(options.resultRoot, project.id, arm, "run-state.json");
      const state = JSON.parse(await readFile(statePath, "utf8"));
      const rolloutName = sessionFiles.find((name) => name.includes(state.threadId));
      if (!rolloutName) throw new Error(`missing rollout for ${project.id}/${arm}`);
      const rolloutPath = path.join(options.sessionsRoot, rolloutName);
      const summary = summarizeLongitudinalEvents(await readEvents(rolloutPath));
      if (summary.issues.length > 0 || summary.turns.length !== project.cases.length) {
        throw new Error(`invalid rollout for ${project.id}/${arm}`);
      }
      arms[arm] = { state, summary, rolloutPath };
    }

    const labels = Object.fromEntries(
      ["A", "B"].map((label) => [label, armForLabel(project.id, options.seed, label)]),
    );
    const benchmarkCase = project.cases[0];
    const statefulTurn = arms.stateful.summary.turns[0];
    const packet = {
      rubricVersion: RUBRIC_VERSION,
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
      durableState: {
        label: "S",
        instructions:
          "Score this persistent record independently for semantic fidelity, evidence traceability, compression value, uncertainty preservation, and future usability. It is not a third visible answer.",
        value: redactArmPaths(
          statefulTurn.durableCompletion?.submittedResult ?? "",
          options.snapshotRoot,
          project.id,
        ),
      },
    };
    mapping.projects[project.id] = {
      labels,
      rollouts: {
        baseline: arms.baseline.rolloutPath,
        stateful: arms.stateful.rolloutPath,
      },
    };
    await writeFile(
      path.join(options.output, `${project.id}.packet.json`),
      `${JSON.stringify(packet, null, 2)}\n`,
    );
  }

  await writeFile(
    path.join(options.output, "grading-map.json"),
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
    else if (argument === "--seed") options.seed = value;
    else throw new Error(`unknown argument: ${argument}`);
  }
  const required = [
    "manifest",
    "resultRoot",
    "snapshotRoot",
    "sessionsRoot",
    "output",
    "seed",
  ];
  if (required.some((field) => !options[field])) {
    throw new Error(
      "usage: --manifest PATH --result-root PATH --snapshot-root PATH --sessions-root PATH --output PATH --seed VALUE",
    );
  }
  for (const field of required.filter((field) => field !== "seed")) {
    options[field] = path.resolve(options[field]);
  }
  return options;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
