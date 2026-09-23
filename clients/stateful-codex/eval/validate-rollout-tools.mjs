import { normalizePrompt } from "./compare-rollouts.mjs";

export function validateRolloutToolAssertions({
  summary,
  benchmarkCase,
}) {
  const assertions = benchmarkCase.statefulToolAssertions;
  if (!assertions) return null;
  const turn = summary.turns.at(-1);
  if (!turn || normalizePrompt(turn.userPrompt) !== normalizePrompt(benchmarkCase.prompt)) {
    throw new Error(`could not identify canonical rollout turn for ${benchmarkCase.id}`);
  }
  const requiredScopes = assertions.blackboardEntryScopes ?? [];
  if (!Array.isArray(requiredScopes) || requiredScopes.some((scope) => typeof scope !== "string")) {
    throw new Error(`${benchmarkCase.id} blackboardEntryScopes must be strings`);
  }
  const missing = requiredScopes.filter(
    (scope) => !turn.calls.blackboardEntryScopes.includes(scope),
  );
  if (missing.length > 0) {
    throw new Error(
      `${benchmarkCase.id} did not query required blackboard entry scopes: ${missing.join(", ")}`,
    );
  }
  return {
    blackboardEntryScopes: turn.calls.blackboardEntryScopes,
    requiredBlackboardEntryScopes: requiredScopes,
  };
}
