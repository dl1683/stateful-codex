import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";

import {
  compareSummaries,
  normalizePrompt,
  readEvents,
  summarizeEvents,
} from "./compare-rollouts.mjs";

export function compareSeries(
  manifest,
  rollouts,
  maturationUsage = manifest.maturationUsage,
) {
  const cases = manifest.cases.map((benchmarkCase) => {
    const paths = rollouts.get(benchmarkCase.id);
    if (!paths) throw new Error(`missing rollout pair for ${benchmarkCase.id}`);
    const comparison = compareSummaries(paths.baseline, paths.stateful);
    const promptMatchesManifest =
      normalizePrompt(comparison.baseline.userPrompt) ===
      normalizePrompt(benchmarkCase.prompt);
    const baselineAnswer = scoreAnswer(comparison.baseline.finalAnswer, benchmarkCase);
    const statefulAnswer = scoreAnswer(comparison.stateful.finalAnswer, benchmarkCase);
    const statefulDurableCompletion = benchmarkCase.requireDurableCompletion
      ? scoreAnswer(
          comparison.stateful.durableCompletion?.coverageText ?? "",
          benchmarkCase,
        )
      : null;
    return {
      id: benchmarkCase.id,
      promptMatchesManifest,
      comparable: comparison.comparable,
      baselineAnswer,
      statefulAnswer,
      statefulDurableCompletion,
      baseline: compactSummary(comparison.baseline),
      stateful: compactSummary(comparison.stateful),
      delta: comparison.delta,
      passed:
        promptMatchesManifest &&
        comparison.comparable &&
        baselineAnswer.passed &&
        statefulAnswer.passed &&
        (statefulDurableCompletion?.passed ?? true),
    };
  });

  const baseline = sumCases(cases, "baseline");
  const followUps = sumCases(cases, "stateful");
  const maturation = maturationUsage;
  const statefulLifetime = addUsage(followUps, maturation);
  const perQuestionSavings = {
    totalTokens: baseline.totalTokens - followUps.totalTokens,
    uncachedTotalTokens:
      baseline.uncachedTotalTokens - followUps.uncachedTotalTokens,
  };

  return {
    name: manifest.name,
    passed: cases.every((benchmarkCase) => benchmarkCase.passed),
    cases,
    aggregate: {
      baseline,
      statefulFollowUps: followUps,
      maturation,
      statefulLifetime,
      lifetimeDelta: {
        totalTokens: statefulLifetime.totalTokens - baseline.totalTokens,
        uncachedTotalTokens:
          statefulLifetime.uncachedTotalTokens - baseline.uncachedTotalTokens,
      },
      followUpWins: {
        totalTokens: countWins(cases, "totalTokens"),
        uncachedTotalTokens: countWins(cases, "uncachedTotalTokens"),
      },
      projectedBreakEvenQuestions: {
        totalTokens: projectedBreakEven(
          maturation.totalTokens,
          perQuestionSavings.totalTokens / cases.length,
        ),
        uncachedTotalTokens: projectedBreakEven(
          maturation.uncachedTotalTokens,
          perQuestionSavings.uncachedTotalTokens / cases.length,
        ),
      },
    },
  };
}

export function scoreAnswer(answer, benchmarkCase) {
  const normalized = answer.toLocaleLowerCase("en-US");
  const concepts = benchmarkCase.expectedConcepts.map((concept) => ({
    name: concept.name,
    matched: concept.termGroups.every((alternatives) =>
      alternatives.some((term) =>
        normalized.includes(term.toLocaleLowerCase("en-US")),
      ),
    ),
  }));
  const forbiddenClaims = benchmarkCase.forbiddenPhrases.filter((phrase) =>
    normalized.includes(phrase.toLocaleLowerCase("en-US")),
  );
  return {
    concepts,
    forbiddenClaims,
    passed:
      concepts.every((concept) => concept.matched) && forbiddenClaims.length === 0,
  };
}

function compactSummary(summary) {
  return {
    sessionId: summary.sessionId,
    usage: summary.usage,
    modelResponses: summary.modelResponses,
    durableCompletion: summary.durableCompletion
      ? {
          runId: summary.durableCompletion.runId,
          revision: summary.durableCompletion.revision,
          checklistItems: summary.durableCompletion.checklist.length,
          omittedChecklistItems:
            summary.durableCompletion.omittedChecklistItems,
        }
      : null,
    calls: summary.calls,
  };
}

function sumCases(cases, side) {
  return cases.reduce(
    (total, benchmarkCase) => ({
      totalTokens: total.totalTokens + benchmarkCase[side].usage.totalTokens,
      uncachedTotalTokens:
        total.uncachedTotalTokens +
        benchmarkCase[side].usage.uncachedTotalTokens,
      modelResponses: total.modelResponses + benchmarkCase[side].modelResponses,
      readBearingToolCalls:
        total.readBearingToolCalls +
        benchmarkCase[side].calls.readBearingToolCalls,
    }),
    {
      totalTokens: 0,
      uncachedTotalTokens: 0,
      modelResponses: 0,
      readBearingToolCalls: 0,
    },
  );
}

function addUsage(followUps, maturation) {
  return {
    totalTokens: followUps.totalTokens + maturation.totalTokens,
    uncachedTotalTokens:
      followUps.uncachedTotalTokens + maturation.uncachedTotalTokens,
    modelResponses: followUps.modelResponses + maturation.modelResponses,
    readBearingToolCalls: followUps.readBearingToolCalls,
  };
}

function countWins(cases, metric) {
  return cases.filter(
    (benchmarkCase) =>
      benchmarkCase.stateful.usage[metric] < benchmarkCase.baseline.usage[metric],
  ).length;
}

function projectedBreakEven(maturationCost, averageSavings) {
  if (averageSavings <= 0) return null;
  return Math.ceil(maturationCost / averageSavings);
}

function parseArgs(args) {
  const options = { pairs: [], maturationRollouts: [] };
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    const value = args[index + 1];
    if (argument === "--manifest") options.manifest = value;
    else if (argument === "--pair") options.pairs.push(value);
    else if (argument === "--maturation-rollout")
      options.maturationRollouts.push(value);
    else throw new Error(`unknown argument: ${argument}`);
    index += 1;
  }
  if (!options.manifest || options.pairs.length === 0) {
    throw new Error(
      "usage: --manifest PATH [--maturation-rollout PATH ...] --pair CASE=BASELINE,STATEFUL [--pair ...]",
    );
  }
  return options;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const manifest = JSON.parse(await readFile(options.manifest, "utf8"));
  const rollouts = new Map();
  for (const pair of options.pairs) {
    const [id, paths] = pair.split("=", 2);
    const [baselinePath, statefulPath] = paths?.split(",", 2) ?? [];
    if (!id || !baselinePath || !statefulPath || rollouts.has(id)) {
      throw new Error(`invalid or duplicate rollout pair: ${pair}`);
    }
    const [baselineEvents, statefulEvents] = await Promise.all([
      readEvents(baselinePath),
      readEvents(statefulPath),
    ]);
    rollouts.set(id, {
      baseline: summarizeEvents(baselineEvents),
      stateful: summarizeEvents(statefulEvents),
    });
  }
  let maturationUsage = manifest.maturationUsage;
  if (options.maturationRollouts.length > 0) {
    const maturations = await Promise.all(
      options.maturationRollouts.map(async (path) =>
        summarizeEvents(await readEvents(path)),
      ),
    );
    maturationUsage = maturations.reduce(
      (total, maturation) => ({
        totalTokens: total.totalTokens + maturation.usage.totalTokens,
        uncachedTotalTokens:
          total.uncachedTotalTokens + maturation.usage.uncachedTotalTokens,
        modelResponses: total.modelResponses + maturation.modelResponses,
      }),
      { totalTokens: 0, uncachedTotalTokens: 0, modelResponses: 0 },
    );
  }
  const report = compareSeries(manifest, rollouts, maturationUsage);
  console.log(JSON.stringify(report, null, 2));
  if (!report.passed) process.exitCode = 2;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
