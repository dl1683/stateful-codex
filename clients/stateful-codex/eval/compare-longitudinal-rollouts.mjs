import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";

import { normalizePrompt, readEvents } from "./compare-rollouts.mjs";
import {
  addUsage,
  emptyUsage,
  summarizeLongitudinalEvents,
} from "./summarize-longitudinal-rollout.mjs";
import { validateObservationField } from "./validate-longitudinal-observation.mjs";

export function compareLongitudinalSuite(
  manifest,
  rolloutPairs,
  observationPairs = new Map(),
) {
  const projects = manifest.projects.map((project) => {
    const pair = rolloutPairs.get(project.id);
    if (!pair) throw new Error(`missing rollout pair for ${project.id}`);
    return compareProject(
      project,
      summarizeLongitudinalEvents(pair.baseline),
      summarizeLongitudinalEvents(pair.stateful),
      observationPairs.get(project.id) ?? {},
      manifest.requiredObservations,
    );
  });
  return {
    name: manifest.name,
    valid: projects.every((project) => project.valid),
    projects,
    aggregate: aggregateProjects(projects),
    inferenceBoundary:
      "Projects are the clustered units. Turn totals are descriptive and must not be treated as independent samples.",
  };
}

function compareProject(
  project,
  baseline,
  stateful,
  observations,
  suiteRequirements,
) {
  const protocolErrors = [...baseline.issues, ...stateful.issues];
  recordCountError(protocolErrors, "baseline compactions", baseline.unattributedCompactions);
  recordCountError(protocolErrors, "stateful compactions", stateful.unattributedCompactions);
  recordExpectedTurns(protocolErrors, "baseline", baseline.turns.length, project.cases.length);
  recordExpectedTurns(protocolErrors, "stateful", stateful.turns.length, project.cases.length);
  const cumulative = { baseline: emptyUsage(), stateful: emptyUsage() };
  const requirements = project.requiredObservations ?? suiteRequirements ?? {};
  const cases = project.cases.map((benchmarkCase, index) => {
    const baselineTurn = baseline.turns[index] ?? null;
    const statefulTurn = stateful.turns[index] ?? null;
    const baselineObservation = observations.baseline?.cases?.[benchmarkCase.id] ?? null;
    const statefulObservation = observations.stateful?.cases?.[benchmarkCase.id] ?? null;
    const missingMeasurements = [
      ...missingObservationFields("baseline", baselineObservation, requirements.baseline),
      ...missingObservationFields("stateful", statefulObservation, requirements.stateful),
    ];
    if (baselineTurn) addUsage(cumulative.baseline, baselineTurn.usage);
    if (statefulTurn) addUsage(cumulative.stateful, statefulTurn.usage);
    const parity = compareTurnParity(
      project,
      baseline,
      stateful,
      baselineTurn,
      statefulTurn,
      baselineObservation,
      statefulObservation,
    );
    return {
      id: benchmarkCase.id,
      promptMatches: {
        baseline: promptMatches(baselineTurn, benchmarkCase.prompt),
        stateful: promptMatches(statefulTurn, benchmarkCase.prompt),
      },
      comparable: Object.values(parity).every(Boolean),
      parity,
      missingMeasurements,
      baseline: observedTurn(baselineTurn, baselineObservation, benchmarkCase),
      stateful: observedTurn(statefulTurn, statefulObservation, benchmarkCase),
      delta: compareTurnUsage(baselineTurn, statefulTurn),
      cumulative: {
        baseline: { ...cumulative.baseline },
        stateful: { ...cumulative.stateful },
        delta: usageDelta(cumulative.baseline, cumulative.stateful),
      },
    };
  });
  for (const benchmarkCase of cases) validateCase(protocolErrors, project, benchmarkCase);
  return {
    id: project.id,
    valid: protocolErrors.length === 0,
    protocolErrors,
    baselineSessionId: baseline.sessionId,
    statefulSessionId: stateful.sessionId,
    cases,
    aggregate: aggregateCases(cases),
  };
}

function validateCase(errors, project, benchmarkCase) {
  if (!benchmarkCase.promptMatches.baseline || !benchmarkCase.promptMatches.stateful) {
    errors.push(`prompt mismatch for ${benchmarkCase.id}`);
  }
  if (!benchmarkCase.comparable) errors.push(`configuration mismatch for ${benchmarkCase.id}`);
  if (!benchmarkCase.baseline?.complete || !benchmarkCase.stateful?.complete) {
    errors.push(`incomplete turn for ${benchmarkCase.id}`);
  }
  for (const issue of benchmarkCase.baseline?.measurementIssues ?? []) {
    errors.push(`${benchmarkCase.id}: baseline ${issue}`);
  }
  for (const issue of benchmarkCase.stateful?.measurementIssues ?? []) {
    errors.push(`${benchmarkCase.id}: stateful ${issue}`);
  }
  if (
    project.requireStatefulProjectState !== false &&
    benchmarkCase.stateful?.projectState.atFirstResponse == null
  ) {
    errors.push(`${benchmarkCase.id}: Stateful project state was not model-visible`);
  }
  if (
    project.requireDurableCompletion !== false &&
    benchmarkCase.stateful?.durableCompletion == null
  ) {
    errors.push(`${benchmarkCase.id}: durable completion was not observed`);
  }
  for (const missing of benchmarkCase.missingMeasurements) {
    errors.push(`${benchmarkCase.id}: missing ${missing}`);
  }
}

function recordCountError(errors, label, count) {
  if (count > 0) errors.push(`${label} has ${count} unattributed records`);
}

function recordExpectedTurns(errors, arm, actual, expected) {
  if (actual !== expected) errors.push(`${arm} has ${actual} turns; expected ${expected}`);
}

function promptMatches(turn, prompt) {
  return turn != null && normalizePrompt(turn.userPrompt) === normalizePrompt(prompt);
}

function observedTurn(turn, observation, benchmarkCase) {
  if (!turn) return null;
  return {
    ...turn,
    diagnosticAnswerCheck: diagnosticAnswerCheck(turn.finalAnswer, benchmarkCase),
    observation,
  };
}

function diagnosticAnswerCheck(answer, benchmarkCase) {
  const expectedConcepts = benchmarkCase.expectedConcepts ?? [];
  const forbiddenPhrases = benchmarkCase.forbiddenPhrases ?? [];
  if (expectedConcepts.length === 0 && forbiddenPhrases.length === 0) return null;
  const normalized = answer.toLocaleLowerCase("en-US");
  const concepts = expectedConcepts.map((concept) => ({
    name: concept.name,
    matched: concept.termGroups.every((alternatives) =>
      alternatives.some((term) => normalized.includes(term.toLocaleLowerCase("en-US"))),
    ),
  }));
  const forbiddenClaims = forbiddenPhrases.filter((phrase) =>
    normalized.includes(phrase.toLocaleLowerCase("en-US")),
  );
  return {
    diagnosticOnly: true,
    concepts,
    forbiddenClaims,
    passed: concepts.every((concept) => concept.matched) && forbiddenClaims.length === 0,
  };
}

function missingObservationFields(arm, observation, required = []) {
  return required.flatMap((field) =>
    validateObservationField(observation, field, arm).map(
      (path) => `${arm} observation.${path}`,
    ),
  );
}

function compareTurnParity(
  project,
  baselineSession,
  statefulSession,
  baseline,
  stateful,
  baselineObservation,
  statefulObservation,
) {
  const left = baseline?.configuration;
  const right = stateful?.configuration;
  const isolatedCopies = project.isolatedCopies === true;
  return {
    originator: sameDefined(baselineSession.originator, statefulSession.originator),
    source: sameDefined(baselineSession.source, statefulSession.source),
    model: sameDefined(left?.model, right?.model),
    reasoningEffort: sameDefined(left?.reasoningEffort, right?.reasoningEffort),
    approvalPolicy: sameDefined(left?.approvalPolicy, right?.approvalPolicy),
    sandboxPolicy: sameDefined(left?.sandboxPolicy, right?.sandboxPolicy),
    permissionProfile: sameDefined(left?.permissionProfile, right?.permissionProfile),
    cwd: isolatedCopies
      ? left?.cwd != null && right?.cwd != null
      : sameDefined(left?.cwd, right?.cwd),
    workspaceRoots: isolatedCopies
      ? left?.workspaceRoots?.length > 0 && right?.workspaceRoots?.length > 0
      : left?.workspaceRoots != null &&
        JSON.stringify(left.workspaceRoots) === JSON.stringify(right?.workspaceRoots),
    corpusRevision:
      !isolatedCopies ||
      sameDefined(
        baselineObservation?.sourceAudit?.corpusRevision,
        statefulObservation?.sourceAudit?.corpusRevision,
      ),
  };
}

function compareTurnUsage(baseline, stateful) {
  if (!baseline || !stateful) return null;
  return {
    usage: usageDelta(baseline.usage, stateful.usage),
    modelResponses: stateful.modelResponses - baseline.modelResponses,
    readOperations: stateful.calls.readOperations - baseline.calls.readOperations,
    rejectedToolResults:
      stateful.calls.rejectedToolResults - baseline.calls.rejectedToolResults,
  };
}

function aggregateCases(cases) {
  const baseline = emptyAggregate();
  const stateful = emptyAggregate();
  for (const benchmarkCase of cases) {
    accumulateTurn(baseline, benchmarkCase.baseline);
    accumulateTurn(stateful, benchmarkCase.stateful);
  }
  return { baseline, stateful, delta: aggregateDelta(baseline, stateful) };
}

function aggregateProjects(projects) {
  const baseline = emptyAggregate();
  const stateful = emptyAggregate();
  for (const project of projects) {
    addAggregate(baseline, project.aggregate.baseline);
    addAggregate(stateful, project.aggregate.stateful);
  }
  return {
    projectCount: projects.length,
    turnCount: projects.reduce((count, project) => count + project.cases.length, 0),
    baseline,
    stateful,
    delta: aggregateDelta(baseline, stateful),
    projectWins: {
      totalTokens: countProjectWins(projects, "totalTokens"),
      uncachedTotalTokens: countProjectWins(projects, "uncachedTotalTokens"),
    },
  };
}

function countProjectWins(projects, metric) {
  return projects.filter(
    (project) =>
      project.aggregate.stateful.usage[metric] < project.aggregate.baseline.usage[metric],
  ).length;
}

function emptyAggregate() {
  return {
    usage: emptyUsage(),
    modelResponses: 0,
    readOperations: 0,
    rejectedToolResults: 0,
    compactions: 0,
    durationMs: 0,
  };
}

function accumulateTurn(total, turn) {
  if (!turn) return;
  addUsage(total.usage, turn.usage);
  total.modelResponses += turn.modelResponses;
  total.readOperations += turn.calls.readOperations;
  total.rejectedToolResults += turn.calls.rejectedToolResults;
  total.compactions += turn.compaction.observed;
  total.durationMs += turn.latency.durationMs ?? 0;
}

function addAggregate(total, addition) {
  addUsage(total.usage, addition.usage);
  for (const field of [
    "modelResponses",
    "readOperations",
    "rejectedToolResults",
    "compactions",
    "durationMs",
  ]) {
    total[field] += addition[field];
  }
}

function aggregateDelta(baseline, stateful) {
  return {
    usage: usageDelta(baseline.usage, stateful.usage),
    modelResponses: stateful.modelResponses - baseline.modelResponses,
    readOperations: stateful.readOperations - baseline.readOperations,
    rejectedToolResults: stateful.rejectedToolResults - baseline.rejectedToolResults,
    durationMs: stateful.durationMs - baseline.durationMs,
  };
}

function usageDelta(baseline, stateful) {
  return Object.fromEntries(
    Object.keys(baseline).map((field) => [field, delta(baseline[field], stateful[field])]),
  );
}

function delta(baseline, stateful) {
  const absolute = stateful - baseline;
  return { absolute, percent: baseline === 0 ? null : (absolute / baseline) * 100 };
}

function sameDefined(left, right) {
  return left != null && right != null && left === right;
}

function parseAssignment(value, label) {
  const [id, paths] = value.split("=", 2);
  const parts = paths?.split(",", 2) ?? [];
  if (!id || parts.length !== 2 || parts.some((part) => !part)) {
    throw new Error(`invalid ${label}: ${value}`);
  }
  return [id, parts];
}

function parseArgs(args) {
  const options = { pairs: [], observations: [] };
  for (let index = 0; index < args.length; index += 2) {
    const [argument, value] = args.slice(index, index + 2);
    if (argument === "--manifest") options.manifest = value;
    else if (argument === "--pair") options.pairs.push(value);
    else if (argument === "--observations") options.observations.push(value);
    else throw new Error(`unknown argument: ${argument}`);
  }
  if (!options.manifest || options.pairs.length === 0) {
    throw new Error(
      "usage: --manifest PATH --pair PROJECT=BASELINE,STATEFUL [--pair ...] [--observations PROJECT=BASELINE,STATEFUL ...]",
    );
  }
  return options;
}

async function readPairs(assignments, reader, label) {
  const pairs = new Map();
  for (const assignment of assignments) {
    const [id, [baselinePath, statefulPath]] = parseAssignment(assignment, label);
    if (pairs.has(id)) throw new Error(`duplicate ${label}: ${id}`);
    const [baseline, stateful] = await Promise.all([
      reader(baselinePath),
      reader(statefulPath),
    ]);
    pairs.set(id, { baseline, stateful });
  }
  return pairs;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const [manifest, rolloutPairs, observationPairs] = await Promise.all([
    readJson(options.manifest),
    readPairs(options.pairs, readEvents, "rollout pair"),
    readPairs(options.observations, readJson, "observation pair"),
  ]);
  const report = compareLongitudinalSuite(manifest, rolloutPairs, observationPairs);
  console.log(JSON.stringify(report, null, 2));
  if (!report.valid) process.exitCode = 2;
}

async function readJson(path) {
  return JSON.parse(await readFile(path, "utf8"));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
